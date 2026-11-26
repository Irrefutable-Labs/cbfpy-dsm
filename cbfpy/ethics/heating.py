"""The care home's heating limit as a CBFpy problem.

State `z = (T,)`: a room's temperature in °C. The control is the rate of heating or cooling. The
safe set is the band of temperatures everyone the setting counts can bear, `lo <= T <= hi`, as two
relative-degree-one barriers `T - lo` and `hi - T`. The enforcer allows a setting only inside that
band, and the cross-check holds it to exactly that: a room driven toward a setting under CBFpy's
safety filter reaches it if and only if the enforcer allows it, and never leaves the band.
"""

import jax.numpy as jnp

from cbfpy.config.cbf_config import CBFConfig


class HeatingConfig(CBFConfig):
    """A room whose temperature must stay inside `[lo, hi]`."""

    def __init__(self, lo, hi):
        self.lo = float(lo)
        self.hi = float(hi)
        super().__init__(n=1, m=1, relax_qp=False, solver_tol=1e-9)

    def f(self, z):
        return jnp.zeros(1)

    def g(self, z):
        return jnp.eye(1)

    def h_1(self, z):
        return jnp.array([z[0] - self.lo, self.hi - z[0]])
