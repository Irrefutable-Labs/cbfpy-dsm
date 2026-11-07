"""The van's braking limit as a CBFpy problem.

State `z = (s, v)`: `s` is the road seen clear ahead, in metres, and `v` the speed, in metres per
second. The control is the acceleration, at least `-a`: the hardest the van can brake. The van
closes on the end of what it has seen, `s' = -v`, and `v' = u`.

The barrier is `h(z) = s - v^2 / (2a)`: the road seen clear, less the distance needed to stop. It
is relative degree one in `u`. The enforcer admits a drive only if this is non-negative at the far
end of the step, in whole cells, and the cross-check holds it to exactly that.
"""

import jax.numpy as jnp

from cbfpy.config.cbf_config import CBFConfig


class BrakingConfig(CBFConfig):
    """A van that must always be able to stop within the road it has seen clear."""

    def __init__(self, braking):
        self.braking = float(braking)
        super().__init__(
            n=2,
            m=1,
            u_min=jnp.array([-self.braking]),
            relax_qp=False,
            solver_tol=1e-9,
        )

    def f(self, z):
        return jnp.array([-z[1], 0.0])

    def g(self, z):
        return jnp.array([[0.0], [1.0]])

    def h_1(self, z):
        return jnp.array([z[0] - z[1] ** 2 / (2.0 * self.braking)])
