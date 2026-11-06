//! The enforcer as a service. It owns the keys, the chain, the DSM head and the world. A client
//! can only ask for the menu and the state, propose, and choose among what it was told is
//! eligible. Only a committed step acts on the world, and only through `act::perform`.

use crate::act::{perform, sweep_partial, EffectReceipt};
use crate::enforcer::{AdapterError, Enforcer, Future, FutureStatus, StepRecord};
use crate::hexser::{decode, to_hex};
use crate::protocol::{receive, send, MenuItem, Position, Request, Response};
use crate::report::report;
use crate::store::ChainStore;
use crate::verify::verify_step;
use std::io::BufReader;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;

pub struct Service {
    enforcer: Enforcer,
    store: ChainStore,
    world: PathBuf,
    trusted_authority: Vec<u8>,
    /// The fork most recently proposed. Any commit closes it.
    open: Option<Vec<Future>>,
    /// Set when a committed step's act failed. Nothing more commits until a restart finishes
    /// it, so receipts can never fall out of order.
    halted: Option<String>,
    /// When set, only this user may use the socket, and it may not be the enforcer's own.
    agent_uid: Option<u32>,
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
        steps: &[StepRecord],
        receipts: usize,
    ) -> Result<Self, AdapterError> {
        std::fs::create_dir_all(&world).map_err(|e| AdapterError::Effect(e.to_string()))?;
        let swept = sweep_partial(&world)?;
        if swept > 0 {
            eprintln!("removed {swept} temporary files an interrupted act left behind");
        }
        let service = Service {
            enforcer,
            store,
            world,
            trusted_authority,
            open: None,
            halted: None,
            agent_uid: None,
        };
        for step in &steps[receipts..] {
            let receipt = service.act(step)?;
            eprintln!(
                "carried out the act of step {} after the restart",
                receipt.sequence
            );
        }
        Ok(service)
    }

    fn act(&self, step: &StepRecord) -> Result<EffectReceipt, AdapterError> {
        let outputs = perform(
            &self.world,
            step.successor.sequence,
            &step.row.act,
            &step.proposal.text,
        )?;
        let receipt = self.enforcer.sign_receipt(step, outputs)?;
        self.store.append_receipt(&receipt)?;
        Ok(receipt)
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
                    })
                    .collect(),
            },
            Request::Status => {
                let state = self.enforcer.state();
                Response::Status {
                    sequence: state.sequence,
                    tip: to_hex(&state.tip),
                    dsm_root: to_hex(&self.enforcer.dsm_root()),
                    policy_hash: to_hex(&state.policy_hash),
                    positions: self
                        .enforcer
                        .charter()
                        .policy
                        .bands
                        .iter()
                        .zip(&state.positions)
                        .map(|(b, v)| Position {
                            dim: b.name.clone(),
                            value: *v,
                        })
                        .collect(),
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
                let futures = self.enforcer.fork(&proposals);
                let fired_rules = futures
                    .iter()
                    .find_map(|f| match &f.status {
                        FutureStatus::Eligible(e) | FutureStatus::Rejected(e) => {
                            Some(e.fired_rules.clone())
                        }
                        FutureStatus::Unmeasured | FutureStatus::Malformed => None,
                    })
                    .unwrap_or_default();
                let response = Response::Forked {
                    fork_root: to_hex(&futures[0].fork_root),
                    fired_rules,
                    futures: report(&futures, &self.enforcer.charter().policy),
                };
                self.open = Some(futures);
                response
            }
            Request::Choose { fork_root, index } => self.choose(&fork_root, index),
        }
    }

    fn choose(&mut self, fork_root: &str, index: usize) -> Response {
        let Some(futures) = self.open.take() else {
            return Response::Refused {
                reason: "no fork is open".to_string(),
            };
        };
        let root = decode(fork_root).ok();
        let Some(future) = futures
            .get(index)
            .filter(|f| Some(f.fork_root.to_vec()) == root)
        else {
            self.open = Some(futures);
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
                self.open = Some(futures);
                return Response::Refused {
                    reason: "that candidate is not eligible".to_string(),
                };
            }
            Err(e) => {
                return Response::Error {
                    message: e.to_string(),
                }
            }
        };
        // Only now, with the step committed and stored, does anything happen in the world.
        match self.act(&step) {
            Ok(receipt) => Response::Committed {
                sequence: step.successor.sequence,
                action: step.proposal.action.clone(),
                fork_size: step.fork.len(),
                tip: to_hex(&step.successor.tip),
                decision: to_hex(&step.decision_commit),
                verified: true,
                effects: receipt.outputs.iter().map(|o| o.path.clone()).collect(),
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
