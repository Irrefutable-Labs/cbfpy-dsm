"""The DSM enforcer's heating limit against a CBFpy barrier, on the real charter.

`ethical-trace` prints, for each protected species in a room, and for a room no one has read,
whether the enforcer allows each heating setting. The barrier is the band that species can bear,
or, for an unread room, the band every protected species can bear. A room driven toward each
setting under CBFpy's safety filter must reach it exactly when the enforcer allows it, and never
leave the band either way.
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
from cbfpy.ethics import HeatingConfig  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent
CHARTER = ROOT / "dsm" / "scenarios" / "charter.json"


def engine_trace():
    out = subprocess.run(
        ["cargo", "run", "-q", "--release", "-p", "ethical-autonomy-adapter", "--bin",
         "ethical-trace", "--", "--charter", str(CHARTER)],
        cwd=ROOT / "dsm", check=True, capture_output=True, text=True,
    ).stdout
    rows = []
    for line in out.splitlines():
        who, celsius, allowed = line.split()
        rows.append((who, int(celsius), int(allowed)))
    return rows


def bands():
    """Each protected species' band, and the band every one of them can bear."""
    foundation = json.loads(CHARTER.read_text())["policy"]["foundation"]
    protected = [
        s for s in foundation["species"]
        if s["adult_mass_mg"] >= foundation["threshold_mg"]
        or s["name"] in foundation["always_protected"]
    ]
    out = {s["name"]: (s["safe_min_c"], s["safe_max_c"]) for s in protected}
    out["unread"] = (max(lo for lo, _ in out.values()), min(hi for _, hi in out.values()))
    return out


class CrossCheck(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.rows = engine_trace()
        cls.bands = bands()

    def test_the_trace_covers_every_protected_species_and_an_unread_room(self):
        self.assertEqual({who for who, _, _ in self.rows}, set(self.bands))
        self.assertGreater(len(self.rows), 100)

    def test_every_verdict_is_the_barrier(self):
        for who, celsius, allowed in self.rows:
            lo, hi = self.bands[who]
            self.assertEqual(bool(allowed), lo <= celsius <= hi, (who, celsius))

    def test_a_room_driven_to_any_setting_under_the_filter_reaches_it_exactly_when_allowed(self):
        checked = 0
        for who, (lo, hi) in self.bands.items():
            cbf = CBF.from_config(HeatingConfig(lo, hi))
            safe = jax.jit(cbf.safety_filter)
            start = (lo + hi) / 2.0
            for _, celsius, allowed in [r for r in self.rows if r[0] == who]:
                z = jnp.array([start])
                for _ in range(400):
                    u_des = jnp.array([0.5 * (celsius - float(z[0]))])
                    z = z + 0.05 * safe(z, u_des)
                    t = float(z[0])
                    self.assertGreaterEqual(t, lo - 1e-3, (who, celsius))
                    self.assertLessEqual(t, hi + 1e-3, (who, celsius))
                reached = abs(float(z[0]) - celsius) < 0.05
                self.assertEqual(reached, bool(allowed), (who, celsius, float(z[0])))
                checked += 1
        self.assertGreater(checked, 100)


if __name__ == "__main__":
    unittest.main()
