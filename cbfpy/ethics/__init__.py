"""Ethical bands as control barrier functions, read from a DSM ethical-autonomy charter.

The DSM engine in `dsm/engine` decides in integers. This package poses the same bands to CBFpy,
in floats, as an independent check of that engine's mathematics. It decides nothing.
"""

from cbfpy.ethics.bands import BandsConfig, load_charter

__all__ = ["BandsConfig", "load_charter"]
