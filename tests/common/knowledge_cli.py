#!/usr/bin/env python3
"""Stable executable for the temporary knowledge CLI test scenarios."""
import sys
from pathlib import Path

scenario = Path(sys.argv[0]).with_suffix(".py")
exec(compile(scenario.read_text(encoding="utf-8"), str(scenario), "exec"))
