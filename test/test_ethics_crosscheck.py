"""The DSM enforcer's speed limit against CBFpy's braking barrier, on the real charter.

`ethical-trace` prints, for each speed and each length of road seen clear, whether the enforcer
allows the drive. The braking barrier `h = s - v^2 / 2a` must agree on every case, and a van at
every allowed speed, filtered by CBFpy's safety filter, must stop within the road it has seen.
"""

import json
import os
import subprocess
import unittest
from pathlib import Path

os.environ.setdefault("JAX_PLATFORMS", "cpu")
os.environ.setdefault("JAX_ENABLE_X64", "1")

import jax  # noqa: E402
import jax.numpy as jnp  # noqa: E402

from cbfpy import CBF  # noqa: E402
from cbfpy.ethics import BrakingConfig  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent
CHARTER = ROOT / "dsm" / "scenarios" / "charter.json"
CELL_M = 1.0


def engine_trace():
    out = subprocess.run(
        ["cargo", "run", "-q", "--release", "-p", "ethical-autonomy-adapter", "--bin",
         "ethical-trace", "--", "--charter", str(CHARTER)],
        cwd=ROOT / "dsm", check=True, capture_output=True, text=True,
    ).stdout
    return [tuple(int(v) for v in line.split()) for line in out.splitlines() if line.strip()]


class CrossCheck(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        operational = json.loads(CHARTER.read_text())["policy"]["operational"]
        cls.braking = operational["braking_cm_s2"] / 100.0
        cls.config = BrakingConfig(cls.braking)
        cbf = CBF.from_config(cls.config)
        cls.filter = staticmethod(jax.jit(cbf.safety_filter))
        cls.rows = engine_trace()

    def barrier(self, speed_cm_s, seen):
        z = jnp.array([seen * CELL_M, speed_cm_s / 100.0])
        return float(self.config.h_1(z)[0])

    def test_the_sample_has_both_verdicts_at_every_length_of_road(self):
        self.assertGreater(len(self.rows), 500)
        # Past 22.5 m, even the top speed of 15 m/s can stop, so only "allowed" is left.
        for seen in range(1, 23):
            verdicts = {a for (_, d, a) in self.rows if d == seen}
            self.assertEqual(verdicts, {0, 1}, seen)

    def test_every_verdict_matches_the_braking_barrier(self):
        for speed, seen, allowed in self.rows:
            # Integer cm, so the barrier's boundary is exact: allowed iff it can stop in time.
            safe = self.barrier(speed, seen) >= -1e-12
            self.assertEqual(bool(allowed), safe, (speed, seen))

    def test_a_van_at_any_allowed_speed_stays_able_to_stop_within_what_it_has_seen(self):
        # From every allowed state, filtered by CBFpy with no wish to slow down, the van never
        # gets nearer the end of the road it has seen than its own stopping distance.
        dt = 0.005
        checked = 0
        for speed, seen, allowed in self.rows:
            if not allowed or seen % 5 != 0 or speed % 250 != 0:
                continue
            z = jnp.array([seen * CELL_M, speed / 100.0])
            worst = float(self.config.h_1(z)[0])
            for _ in range(int(20.0 / dt)):
                u = self.filter(z, jnp.array([0.0]))
                v = max(float(z[1]) + float(u[0]) * dt, 0.0)
                z = jnp.array([float(z[0]) - float(z[1]) * dt, v])
                worst = min(worst, float(self.config.h_1(z)[0]))
                if v == 0.0:
                    break
            # Within one step's integration error of the barrier, and so never past the road.
            self.assertGreaterEqual(worst, -speed / 100.0 * dt, (speed, seen))
            self.assertGreaterEqual(float(z[0]), -speed / 100.0 * dt, (speed, seen))
            checked += 1
        self.assertGreater(checked, 10)


if __name__ == "__main__":
    unittest.main()
