"""A charter's bands as a CBFpy problem.

Each band is a single integrator with a step of one, `z' = z + u`. Each side of a band has two
barriers: its tension barrier `L - d`, whose class-kappa gain is the band's tension curve, and its
stop barrier `T - d`, with unit gain. Coupling rules that adjust a limit are applied inside the
barrier function, from the state, then clamped to the hard limits as the engine does.

What is not posed here: restoration, forced restoration, and the engine's rule for crossing the
center. The engine is stricter on a crossing than this continuous model.
"""

import json
from pathlib import Path

import jax.numpy as jnp

from cbfpy.config.cbf_config import CBFConfig

BASIS_POINTS = 10_000.0


def load_charter(path):
    """The charter as written: `{"policy": ..., "catalog": ...}`."""
    return json.loads(Path(path).read_text())


class BandsConfig(CBFConfig):
    """All of a charter's bands, with their coupling, as one CBF-QP."""

    def __init__(self, charter):
        policy = charter["policy"]
        self.bands = policy["bands"]
        self.names = [b["name"] for b in self.bands]
        n = len(self.bands)

        self.center = jnp.array([float(b["center"]) for b in self.bands])
        self.limit_pos = jnp.array([float(b["limit_pos"]) for b in self.bands])
        self.limit_neg = jnp.array([float(b["limit_neg"]) for b in self.bands])
        self.hard_pos = jnp.array([float(b["hard_pos"]) for b in self.bands])
        self.hard_neg = jnp.array([float(b["hard_neg"]) for b in self.bands])
        self.stop_fraction = jnp.array([b["no_outward_bp"] / BASIS_POINTS for b in self.bands])

        self.adjustments = []
        targets = set()
        for rule in policy["rules"]:
            effect = rule["effect"]
            if effect["kind"] != "adjust":
                continue
            conditions = [
                (self.names.index(c["dim"]), c["test"], float(c["value"])) for c in rule["when"]
            ]
            target = self.names.index(effect["target"])
            targets.add(target)
            self.adjustments.append((conditions, target, effect["side"], float(effect["delta"])))

        for i, b in enumerate(self.bands):
            if b["curve"]["kind"] == "quadratic" and i in targets:
                # CBFpy's gain sees only h, and a quadratic gain needs the limit, which coupling
                # moves for a target band. Refuse rather than pose the wrong problem.
                raise ValueError(f"{b['name']}: a quadratic band cannot be a coupling target here")
        self.curves = [
            (b["curve"]["kind"], b["curve"]["num"] / b["curve"]["den"], float(b["limit_pos"]),
             float(b["limit_neg"]))
            for b in self.bands
        ]
        super().__init__(n=n, m=n, relax_qp=False, solver_tol=1e-9)

    def f(self, z):
        return jnp.zeros(self.n)

    def g(self, z):
        return jnp.eye(self.n)

    def _holds(self, d, dim, test, value):
        if test == "deviation_at_least":
            return jnp.abs(d[dim]) >= value
        if test == "deviation_at_most":
            return jnp.abs(d[dim]) <= value
        if test == "signed_at_least":
            return d[dim] >= value
        if test == "signed_at_most":
            return d[dim] <= value
        raise ValueError(f"unknown test {test}")

    def limits(self, z):
        """The effective limits at `z`: every fired adjustment summed, then clamped."""
        d = z - self.center
        up = self.limit_pos
        down = self.limit_neg
        for conditions, target, side, delta in self.adjustments:
            fired = jnp.all(jnp.array([self._holds(d, *c) for c in conditions]))
            change = jnp.where(fired, delta, 0.0)
            if side in ("positive", "both"):
                up = up.at[target].add(change)
            if side in ("negative", "both"):
                down = down.at[target].add(change)
        return jnp.clip(up, 0.0, self.hard_pos), jnp.clip(down, 0.0, self.hard_neg)

    def h_1(self, z):
        d = z - self.center
        up, down = self.limits(z)
        return jnp.concatenate(
            [up - d, down + d, up * self.stop_fraction - d, down * self.stop_fraction + d]
        )

    def alpha(self, h):
        n = self.n
        gains = []
        for side in range(2):
            for i, (kind, k, lp, ln) in enumerate(self.curves):
                x = h[side * n + i]
                limit = lp if side == 0 else ln
                # A signed square, so the gain is monotone below zero as CBFpy requires.
                gains.append(k * x if kind == "linear" else k * x * jnp.abs(x) / limit)
        return jnp.concatenate([jnp.array(gains), h[2 * n :]])
