"""The DSM integer engine against CBFpy, on the real charter.

The engine prints its allowances with `ethical-trace`. CBFpy solves a strict QP for the same bands
in floats. The engine floors, so its answer must sit at or just under the solver's.
"""

import os
import subprocess
import unittest
from pathlib import Path

os.environ.setdefault("JAX_PLATFORMS", "cpu")
os.environ.setdefault("JAX_ENABLE_X64", "1")

import jax  # noqa: E402
import jax.numpy as jnp  # noqa: E402

from cbfpy import CBF  # noqa: E402
from cbfpy.ethics import BandsConfig, load_charter  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent
CHARTER = ROOT / "dsm" / "scenarios" / "charter.json"
PUSH = 5000.0


def engine_trace():
    out = subprocess.run(
        ["cargo", "run", "-q", "--release", "-p", "ethical-autonomy-adapter", "--bin",
         "ethical-trace", "--", "--charter", str(CHARTER)],
        cwd=ROOT / "dsm", check=True, capture_output=True, text=True,
    ).stdout
    return [[int(v) for v in line.split()] for line in out.splitlines() if line.strip()]


class CrossCheck(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.config = BandsConfig(load_charter(CHARTER))
        cls.n = cls.config.n
        cbf = CBF.from_config(cls.config)
        cls.filter = staticmethod(jax.jit(jax.vmap(cbf.safety_filter)))
        cls.rows = engine_trace()

    def test_the_sample_covers_every_band_and_every_adjusting_rule(self):
        self.assertEqual(self.n, 6)
        self.assertGreater(len(self.rows), 1000)
        fired = set()
        for row in self.rows:
            z = jnp.array([float(v) for v in row[: self.n]])
            up, down = self.config.limits(z)
            for i in range(self.n):
                if float(up[i]) != float(self.config.limit_pos[i]) or float(down[i]) != float(
                    self.config.limit_neg[i]
                ):
                    fired.add(i)
        # Rule targets in the charter: curiosity (1) and risk_reward (3).
        self.assertEqual(fired, {1, 3})

    def test_every_allowance_matches_the_barrier_solver(self):
        n = self.n
        parents = jnp.array([[float(v) for v in row[:n]] for row in self.rows])
        worst, compared = 0.0, 0
        for i in range(n):
            for sign, column in ((1.0, n + 2 * i), (-1.0, n + 2 * i + 1)):
                push = jnp.zeros((len(self.rows), n)).at[:, i].set(sign * PUSH)
                safe = self.filter(parents, push)
                for row, u in zip(self.rows, safe):
                    deviation = row[i] - int(self.config.center[i])
                    # Only the same side is comparable: the engine is stricter on a crossing.
                    if sign * deviation < 0:
                        continue
                    engine, solver = row[column], sign * float(u[i])
                    gap = solver - engine
                    worst = max(worst, abs(gap))
                    compared += 1
                    self.assertGreaterEqual(gap, -1e-3, (row, i, sign, engine, solver))
                    self.assertLess(gap, 1.0 + 1e-3, (row, i, sign, engine, solver))
        self.assertGreater(compared, 5000)
        print(f"\ncompared {compared} allowances, widest gap {worst:.6f} of a unit")

    def test_a_quadratic_band_that_coupling_moves_is_refused(self):
        charter = load_charter(CHARTER)
        charter["policy"]["bands"][1]["curve"]["kind"] = "quadratic"
        with self.assertRaises(ValueError):
            BandsConfig(charter)


if __name__ == "__main__":
    unittest.main()
