"""Manager for polling and coordinating thermal thresholds with background jobs."""

import logging
import threading
from typing import Any, Dict, Optional

from .thermal import ThermalSensor

logger = logging.getLogger(__name__)


class NASManager:
    """Coordinates NAS monitoring, thermal limits, and background worker throttling."""

    def __init__(
        self,
        analysis_jobs,
        gallery_imports=None,
        high_temp: float = 75.0,
        low_temp: float = 60.0,
        interval: float = 5.0,
        enabled: bool = False,
    ) -> None:
        self.analysis_jobs = analysis_jobs
        self.gallery_imports = gallery_imports
        self.high_temp = high_temp
        self.low_temp = low_temp
        self.interval = interval
        self.enabled = enabled

        self.sensor = ThermalSensor()
        self.current_temp = 35.0
        self.status = "normal"  # "normal" or "throttled"
        self._paused_by_thermal = False

        self._lock = threading.Lock()
        self._thread: Optional[threading.Thread] = None
        self._stop_event = threading.Event()

    def start(self) -> None:
        """Start the background thermal polling thread if enabled."""
        with self._lock:
            if not self.enabled:
                return
            if self._thread is not None and self._thread.is_alive():
                return
            self._stop_event.clear()
            self._thread = threading.Thread(target=self._poll_loop, daemon=True, name="nas-thermal-monitor")
            self._thread.start()
            logger.info("NAS Thermal Monitoring started. High Threshold: %s°C, Low: %s°C", self.high_temp, self.low_temp)

    def stop(self) -> None:
        """Stop the background thermal polling thread."""
        with self._lock:
            self._stop_event.set()
            thread = self._thread
            self._thread = None
        if thread is not None:
            thread.join(timeout=2.0)
            logger.info("NAS Thermal Monitoring stopped.")

    def set_config(
        self,
        high_temp: Optional[float] = None,
        low_temp: Optional[float] = None,
        interval: Optional[float] = None,
        enabled: Optional[bool] = None,
    ) -> None:
        """Dynamically update NAS thermal configuration and manage state transitions."""
        with self._lock:
            if high_temp is not None:
                self.high_temp = high_temp
            if low_temp is not None:
                self.low_temp = low_temp
            if interval is not None:
                self.interval = interval
            if enabled is not None:
                was_enabled = self.enabled
                self.enabled = enabled

        # Manage thread changes based on dynamic enable/disable outside lock
        if enabled is not None:
            if enabled:
                self.start()
            else:
                self.stop()
                self._clear_throttle()

    def _clear_throttle(self) -> None:
        with self._lock:
            if self._paused_by_thermal:
                try:
                    self.analysis_jobs.resume()
                except Exception:
                    pass
                self._paused_by_thermal = False
            self.status = "normal"

    def _check_temperature_and_throttle(self) -> None:
        """Read sensor temperature and trigger pause/resume if thresholds are crossed."""
        try:
            temp = self.sensor.read_temperature()
            with self._lock:
                self.current_temp = temp

            # High Temp Trigger: Pause active CPU-heavy analysis
            if temp >= self.high_temp:
                should_pause = False
                with self._lock:
                    if not self._paused_by_thermal:
                        self._paused_by_thermal = True
                        self.status = "throttled"
                        should_pause = True
                if should_pause:
                    logger.warning(
                        "NAS CPU/System temp exceeded threshold: %s°C >= %s°C. Throttling active analysis.",
                        temp,
                        self.high_temp,
                    )
                    try:
                        self.analysis_jobs.pause()
                    except Exception as e:
                        logger.error("Failed to pause analysis job on hot trigger: %s", e)

            # Low Temp Trigger: Resume if cooled down and was paused by thermal
            elif temp <= self.low_temp:
                should_resume = False
                with self._lock:
                    if self._paused_by_thermal:
                        self._paused_by_thermal = False
                        self.status = "normal"
                        should_resume = True
                if should_resume:
                    logger.info(
                        "NAS CPU/System temp returned to safe range: %s°C <= %s°C. Resuming analysis.",
                        temp,
                        self.low_temp,
                    )
                    try:
                        self.analysis_jobs.resume()
                    except Exception as e:
                        logger.error("Failed to resume analysis job on cool trigger: %s", e)

        except Exception as e:
            logger.error("Error in NAS thermal polling: %s", e)

    def _poll_loop(self) -> None:
        while not self._stop_event.is_set():
            self._check_temperature_and_throttle()
            self._stop_event.wait(timeout=self.interval)

    def snapshot(self) -> Dict[str, Any]:
        """Return a serialized representation of current NAS thermal state."""
        with self._lock:
            return {
                "temperature": self.current_temp,
                "status": self.status,
                "high_temp_threshold": self.high_temp,
                "low_temp_threshold": self.low_temp,
                "paused_by_thermal": self._paused_by_thermal,
                "monitoring_enabled": self.enabled,
                "interval": self.interval,
            }
