//! The enforcer as a service. It owns the keys, the chain, the DSM head, the world and the body. A
//! client can only ask for the menu and the state, propose, and choose among what it was told is
//! eligible. Only a committed step acts, and only through `act::perform` or `act::perform_body`.

use crate::act::{perform, perform_body, sweep_partial, EffectReceipt};
use crate::enforcer::{AdapterError, Enforcer, Future, StepRecord, MAX_FORK};
use crate::hexser::{decode, to_hex};
use crate::protocol::{receive, send, MenuItem, Request, Response};
use crate::report::report;
use crate::store::ChainStore;
use crate::verify::verify_step;
use std::io::BufReader;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;

/// The body the enforcer acts through: here, the simulated world, kept on disk so it outlasts a
/// restart. It is the stand-in for physical reality, and the AI never reads it.
pub struct Body {
    path: PathBuf,
    pub world: ethical_world::sim::World,
}

impl Body {
    pub fn open(path: PathBuf) -> Result<Self, AdapterError> {
        let text = std::fs::read_to_string(&path)
            .map_err(|e| AdapterError::Config(format!("{}: {e}", path.display())))?;
        let world = serde_json::from_str(&text).map_err(|e| AdapterError::Config(e.to_string()))?;
        Ok(Body { path, world })
    }

    fn save(&self) -> Result<(), AdapterError> {
        let bytes = serde_json::to_vec_pretty(&self.world)
            .map_err(|e| AdapterError::Effect(e.to_string()))?;
        let temporary = self.path.with_extension("partial");
        {
            let mut file = std::fs::File::create(&temporary)
                .map_err(|e| AdapterError::Effect(e.to_string()))?;
            std::io::Write::write_all(&mut file, &bytes)
                .and_then(|_| file.sync_all())
                .map_err(|e| AdapterError::Effect(e.to_string()))?;
        }
        std::fs::rename(&temporary, &self.path).map_err(|e| AdapterError::Effect(e.to_string()))
    }
}

pub struct Service {
    enforcer: Enforcer,
    store: ChainStore,
    world: PathBuf,
    trusted_authority: Vec<u8>,
    /// The fork most recently put forward. If another replaces it before a choice, it is
    /// signed and stored as considered, so nothing the AI put forward is lost.
    open: Option<Vec<Future>>,
    /// Set when a committed step's act failed. Nothing more commits until a restart finishes
    /// it, so receipts can never fall out of order.
    halted: Option<String>,
    /// When set, only this user may use the socket, and it may not be the enforcer's own.
    agent_uid: Option<u32>,
    body: Option<Body>,
}

impl Service {
    /// Start serving. Any committed step whose act has no receipt yet, which can only follow a
    /// stop between committing and acting, has its act carried out first. Acts are idempotent,
    /// so this writes the same bytes the interrupted act would have.
    pub fn start(
        enforcer: Enforcer,
        store: ChainStore,
        world: PathBuf,
        trusted_authority: Vec<u8>,
        pending: &[StepRecord],
        body: Option<Body>,
    ) -> Result<Self, AdapterError> {
        std::fs::create_dir_all(&world).map_err(|e| AdapterError::Effect(e.to_string()))?;
        std::fs::set_permissions(&world, std::os::unix::fs::PermissionsExt::from_mode(0o700))
            .map_err(|e| AdapterError::Effect(e.to_string()))?;
        let swept = sweep_partial(&world)?;
        if swept > 0 {
            eprintln!("removed {swept} temporary files an interrupted act left behind");
        }
        let mut service = Service {
            enforcer,
            store,
            world,
            trusted_authority,
            open: None,
            halted: None,
            agent_uid: None,
            body,
        };
        for step in pending {
            let (receipt, _) = service.act(step)?;
            eprintln!(
                "carried out the act of step {} after the restart",
                receipt.sequence
            );
        }
        // A look whose report was written but not yet committed: commit what the senses
        // reported then, from the file its receipt names.
        if service.enforcer.knowledge().awaiting.is_some() {
            let sequence = service.enforcer.state().sequence;
            let path = service.world.join(format!("looks/{sequence:06}.json"));
            let observation = std::fs::read(&path)
                .ok()
                .and_then(|b| serde_json::from_slice(&b).ok())
                .ok_or_else(|| {
                    AdapterError::Effect(format!("the look of step {sequence} left no report"))
                })?;
            let store = &service.store;
            service
                .enforcer
                .record_observation(observation, |r| store.append_observed(r))?;
            eprintln!("committed the look of step {sequence} after the restart");
        }
        Ok(service)
    }

    /// Carry out a committed step's act, sign its receipt, and for a look, commit what was seen.
    fn act(
        &mut self,
        step: &StepRecord,
    ) -> Result<(EffectReceipt, Option<ethical_world::Observation>), AdapterError> {
        let sequence = step.successor.sequence;
        let (outputs, observation) = match (&step.row, &step.proposal.body) {
            (Some(row), _) => (
                perform(&self.world, sequence, &row.act, &step.proposal.text)?,
                None,
            ),
            (None, Some(act)) => {
                let body = self
                    .body
                    .as_mut()
                    .ok_or_else(|| AdapterError::Effect("no body is attached".to_string()))?;
                let done = perform_body(&self.world, &mut body.world, sequence, act)?;
                body.save()?;
                done
            }
            (None, None) => {
                return Err(AdapterError::Effect("a step that does nothing".to_string()))
            }
        };
        let receipt = self.enforcer.sign_receipt(step, outputs)?;
        self.store.append_receipt(&receipt)?;
        if let Some(obs) = &observation {
            let store = &self.store;
            self.enforcer
                .record_observation(obs.clone(), |r| store.append_observed(r))?;
        }
        Ok((receipt, observation))
    }

    /// Serve only `uid`, which must not be the enforcer's own user.
    pub fn only_for(mut self, uid: u32) -> Result<Self, AdapterError> {
        if uid == crate::peer::own_uid() {
            return Err(AdapterError::Config(
                "the agent's user must not be the enforcer's own".to_string(),
            ));
        }
        self.agent_uid = Some(uid);
        Ok(self)
    }

    pub fn handle(&mut self, request: Request) -> Response {
        if let (Some(why), Request::Propose { .. } | Request::Choose { .. }) =
            (&self.halted, &request)
        {
            return Response::Refused {
                reason: format!(
                    "halted: {why}. Restart the enforcer to finish it before anything else commits"
                ),
            };
        }
        match request {
            Request::Menu => Response::Menu {
                actions: self
                    .enforcer
                    .charter()
                    .catalog
                    .rows
                    .iter()
                    .map(|r| MenuItem {
                        action: r.action.clone(),
                        description: r.description.clone(),
                        needs_text: r.act.needs_text(),
                        effects: crate::disclosure::described(&r.act).0,
                        affected: crate::disclosure::described(&r.act).1,
                        material: r.material.clone(),
                        must_assert: r.must_assert.clone(),
                    })
                    .collect(),
                facts: self.enforcer.charter().catalog.facts.clone(),
                has_body: self.body.is_some(),
                safe_temperatures: {
                    let f = &self.enforcer.charter().policy.foundation;
                    f.species
                        .iter()
                        .filter(|s| f.is_protected(&s.name))
                        .map(|s| (s.name.clone(), s.safe_min_c, s.safe_max_c))
                        .collect()
                },
            },
            Request::Status => {
                let state = self.enforcer.state();
                Response::Status {
                    sequence: state.sequence,
                    tip: to_hex(&state.tip),
                    dsm_root: to_hex(&self.enforcer.dsm_root()),
                    policy_hash: to_hex(&state.policy_hash),
                    knowledge: state.knowledge,
                }
            }
            Request::Propose { proposals } => {
                if let Some(bad) = proposals.iter().find(|p| p.check().is_err()) {
                    return Response::Refused {
                        reason: format!("a proposal is out of bounds: {}", bad.action),
                    };
                }
                if proposals.is_empty() {
                    return Response::Refused {
                        reason: "nothing was proposed".to_string(),
                    };
                }
                if self.body.is_none() && proposals.iter().any(|p| p.body.is_some()) {
                    return Response::Refused {
                        reason: "no body is attached, so no body act can be carried out"
                            .to_string(),
                    };
                }
                if proposals.len() > MAX_FORK {
                    return Response::Refused {
                        reason: format!(
                            "no more than {MAX_FORK} candidates may be put forward at once"
                        ),
                    };
                }
                if let Some(previous) = self.open.take() {
                    let kept = self
                        .enforcer
                        .sign_considered(previous[0].fork.clone())
                        .and_then(|c| self.store.append_considered(&c));
                    if let Err(e) = kept {
                        let why = format!("a superseded fork could not be stored: {e}");
                        self.halted = Some(why.clone());
                        return Response::Error {
                            message: format!("{why}. Nothing more commits until a restart"),
                        };
                    }
                }
                let futures = self.enforcer.fork(&proposals);
                let response = Response::Forked {
                    fork_root: to_hex(&futures[0].fork_root),
                    futures: report(&futures),
                };
                self.open = Some(futures);
                response
            }
            Request::Choose { fork_root, index } => self.choose(&fork_root, index),
        }
    }

    fn choose(&mut self, fork_root: &str, index: usize) -> Response {
        let Some(futures) = self.open.clone() else {
            return Response::Refused {
                reason: "no fork is open".to_string(),
            };
        };
        let root = decode(fork_root).ok();
        let Some(future) = futures
            .get(index)
            .filter(|f| Some(f.fork_root.to_vec()) == root)
        else {
            return Response::Refused {
                reason: "no such candidate in the open fork".to_string(),
            };
        };
        let parent = self.enforcer.state();
        let charter = self.enforcer.charter().clone();
        let device = self.enforcer.device_public().to_vec();
        let trusted = self.trusted_authority.clone();
        let store = &self.store;
        // The enforcer checks its own step from its parent, then writes it, before installing it.
        let result = self.enforcer.commit_with(future, |step| {
            verify_step(&parent, step, &charter, &trusted, &device)
                .map_err(|r| AdapterError::BadStore(format!("own step did not verify: {r:?}")))?;
            store.append(step)
        });
        let step = match result {
            Ok(step) => step,
            Err(AdapterError::NotEligible) => {
                return Response::Refused {
                    reason: "that candidate is not eligible".to_string(),
                };
            }
            Err(e) => {
                // The step may or may not be on disk. Nothing more commits until a restart
                // reads the store and settles which.
                let why = format!("a step could not be stored: {e}");
                self.halted = Some(why.clone());
                return Response::Error {
                    message: format!("{why}. Nothing more commits until a restart"),
                };
            }
        };
        self.open = None;
        // Only now, with the step committed and stored, does anything happen in the world.
        match self.act(&step) {
            Ok((receipt, observed)) => Response::Committed {
                sequence: step.successor.sequence,
                action: step.proposal.action.clone(),
                fork_size: step.fork.len(),
                tip: to_hex(&step.successor.tip),
                decision: to_hex(&step.decision_commit),
                effects: receipt.outputs.iter().map(|o| o.path.clone()).collect(),
                observed,
            },
            Err(e) => {
                let why = format!(
                    "the act of committed step {} failed: {e}",
                    step.successor.sequence
                );
                self.halted = Some(why.clone());
                Response::Error {
                    message: format!("{why}. Nothing more commits until a restart finishes it"),
                }
            }
        }
    }

    fn connection(&mut self, stream: UnixStream) -> std::io::Result<()> {
        let mut writer = stream.try_clone()?;
        if let Some(agent) = self.agent_uid {
            let peer = crate::peer::peer_uid(&stream)?;
            if peer != agent {
                send(
                    &mut writer,
                    &Response::Refused {
                        reason: format!("this enforcer serves only user {agent}, not user {peer}"),
                    },
                )?;
                return Ok(());
            }
        }
        let mut reader = BufReader::new(stream);
        while let Some(request) = receive::<Request>(&mut reader)? {
            let response = self.handle(request);
            send(&mut writer, &response)?;
        }
        Ok(())
    }

    /// Serve one client at a time, for as long as the process runs.
    pub fn serve(mut self, listener: UnixListener) -> std::io::Result<()> {
        for stream in listener.incoming() {
            match stream {
                Ok(s) => {
                    if let Err(e) = self.connection(s) {
                        eprintln!("connection ended: {e}");
                    }
                }
                Err(e) => eprintln!("accept failed: {e}"),
            }
        }
        Ok(())
    }
}
