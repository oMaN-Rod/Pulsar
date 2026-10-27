//! Readings from the optional sensor service.

use crate::metric::{MetricKey, PerCoreKey, Snapshot, Source, SourceError, SourceId, Timing};
use crate::sensors::{self, PerCore, Sensor, Unavailable};

pub struct SensorServiceSource {
    /// Off when the readings only add detail to a popup: a missing service
    /// is then not worth reporting.
    pub required: bool,
}

impl Source for SensorServiceSource {
    fn id(&self) -> SourceId {
        SourceId::SensorService
    }

    fn sample(&mut self, out: &mut Snapshot) -> Result<(), SourceError> {
        let readings = match sensors::read() {
            Ok(readings) => readings,
            Err(Unavailable::NotInstalled) if !self.required => return Ok(()),
            Err(e) => return Err(SourceError::Unavailable(e.to_string())),
        };
        for (sensor, key) in [
            (Sensor::CpuTemp, MetricKey::CpuTempC),
            (Sensor::CpuPower, MetricKey::CpuPowerW),
            (Sensor::CpuThrottling, MetricKey::CpuThrottling),
            (Sensor::GpuHotSpot, MetricKey::GpuHotSpotC),
            (Sensor::GpuMemoryTemp, MetricKey::GpuMemTempC),
            (Sensor::CpuCoresPower, MetricKey::CpuCoresPowerW),
            (Sensor::CpuGraphicsPower, MetricKey::CpuGraphicsPowerW),
            (Sensor::CpuDramPower, MetricKey::CpuDramPowerW),
            (Sensor::ChipsetTemp, MetricKey::ChipsetTempC),
            (Sensor::CpuTdp, MetricKey::CpuTdpW),
            (Sensor::CpuPowerLimit1, MetricKey::CpuPowerLimit1W),
            (Sensor::CpuPowerLimit2, MetricKey::CpuPowerLimit2W),
            (Sensor::CpuMinRatio, MetricKey::CpuMinRatio),
            (Sensor::CpuMaxRatio, MetricKey::CpuMaxRatio),
            (Sensor::RingClock, MetricKey::RingClockMhz),
            (Sensor::MemoryClock, MetricKey::MemClockMhz),
            (
                Sensor::MemoryControllerClock,
                MetricKey::MemControllerClockMhz,
            ),
            (Sensor::MemoryCl, MetricKey::MemTiming(Timing::Cl)),
            (Sensor::MemoryRcd, MetricKey::MemTiming(Timing::Rcd)),
            (Sensor::MemoryRp, MetricKey::MemTiming(Timing::Rp)),
            (Sensor::MemoryRas, MetricKey::MemTiming(Timing::Ras)),
            (Sensor::MemoryRc, MetricKey::MemTiming(Timing::Rc)),
            (
                Sensor::MemoryCommandRate,
                MetricKey::MemTiming(Timing::CommandRate),
            ),
        ] {
            if let Some(value) = readings.get(sensor) {
                out.set(key, value);
            }
        }
        let keys: [(PerCore, PerCoreKey); 4] = [
            (PerCore::Temp, MetricKey::CpuCoreTempC),
            (PerCore::Clock, MetricKey::CpuCoreClockMhz),
            (PerCore::Voltage, MetricKey::CpuCoreVolts),
            (PerCore::Power, MetricKey::CpuCorePowerW),
        ];
        for (which, key) in keys {
            for (core, value) in readings.per_core(which).into_iter().enumerate() {
                out.set(key(core as u16), value);
            }
        }
        Ok(())
    }
}
