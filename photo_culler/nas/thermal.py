"""Utility and logic for querying host temperature sensors on NAS / Linux."""

from pathlib import Path
from typing import Callable, List, Optional


class ThermalSensor:
    """Utility class to dynamically read CPU/system temperatures on Linux systems."""

    def __init__(self, mock_provider: Optional[Callable[[], float]] = None) -> None:
        self._mock_provider = mock_provider
        self.thermal_paths: List[Path] = [
            Path("/sys/class/thermal"),
            Path("/sys/class/hwmon"),
        ]

    def set_mock_provider(self, provider: Optional[Callable[[], float]]) -> None:
        """Inject a custom mock provider to return synthetic temperatures for testing."""
        self._mock_provider = provider

    def read_temperature(self) -> float:
        """Read the highest available system temperature in Celsius.

        Returns 35.0 as a default fallback if no sensors are discovered.
        """
        if self._mock_provider is not None:
            return self._mock_provider()

        temps: List[float] = []

        # 1. Look in standard Linux /sys/class/thermal/thermal_zone*/temp
        thermal_dir = self.thermal_paths[0]
        if thermal_dir.exists() and thermal_dir.is_dir():
            try:
                for zone in thermal_dir.glob("thermal_zone*"):
                    temp_file = zone / "temp"
                    if temp_file.exists():
                        try:
                            val = float(temp_file.read_text().strip())
                            # Convert milli-degrees Celsius to degrees
                            if val > 1000:
                                val /= 1000.0
                            if -40.0 <= val <= 150.0:  # Sensible safety boundaries
                                temps.append(val)
                        except (ValueError, OSError, PermissionError):
                            pass
            except OSError:
                pass

        # 2. Look in hardware monitors /sys/class/hwmon/hwmon*/temp*_input
        hwmon_dir = self.thermal_paths[1]
        if hwmon_dir.exists() and hwmon_dir.is_dir():
            try:
                for hw in hwmon_dir.glob("hwmon*"):
                    for temp_input in hw.glob("temp*_input"):
                        if temp_input.exists():
                            try:
                                val = float(temp_input.read_text().strip())
                                if val > 1000:
                                    val /= 1000.0
                                if -40.0 <= val <= 150.0:
                                    temps.append(val)
                            except (ValueError, OSError, PermissionError):
                                pass
            except OSError:
                pass

        if temps:
            return max(temps)

        # 3. Fallback default
        return 35.0
