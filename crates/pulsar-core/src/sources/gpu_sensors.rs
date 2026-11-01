//! GPU temperature, fan and power from the display driver, as Task Manager
//! reads them: no admin rights, no vendor libraries.

use windows::Wdk::Graphics::Direct3D::{
    D3DKMT_ADAPTER_PERFDATA, D3DKMT_ADAPTERINFO, D3DKMT_CLOSEADAPTER, D3DKMT_ENUMADAPTERS2,
    D3DKMT_QUERYADAPTERINFO, D3DKMTCloseAdapter, D3DKMTEnumAdapters2, D3DKMTQueryAdapterInfo,
    KMTQAITYPE_ADAPTERPERFDATA,
};

use crate::metric::{MetricKey, Snapshot, Source, SourceError, SourceId};

/// Raw driver units: tenths of a degree, tenths of a percent.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SensorReading {
    pub temp_deci_c: u32,
    pub fan_rpm: u32,
    pub power_deci_percent: u32,
    pub mem_clock_hz: u64,
}

/// The hottest adapter that reports a temperature at all; integrated and
/// software adapters report zero.
pub fn pick_reading(readings: &[SensorReading]) -> Option<SensorReading> {
    readings
        .iter()
        .copied()
        .filter(|r| r.temp_deci_c > 0)
        .max_by_key(|r| r.temp_deci_c)
}

fn publish(r: &SensorReading, out: &mut Snapshot) {
    out.set(MetricKey::GpuTempC, f64::from(r.temp_deci_c) / 10.0);
    out.set(MetricKey::GpuFanRpm, f64::from(r.fan_rpm));
    out.set(
        MetricKey::GpuPowerPercent,
        f64::from(r.power_deci_percent) / 10.0,
    );
    out.set(
        MetricKey::GpuMemClockMhz,
        r.mem_clock_hz as f64 / 1_000_000.0,
    );
}

fn unavailable() -> SourceError {
    SourceError::Unavailable(crate::tr!("GPU temperature unavailable").into())
}

/// Holds the adapter handles open, closing them on drop.
pub struct GpuSensorsSource {
    adapters: Vec<u32>,
    /// Each handle's place in `hardware::gpu::names`, for its own temperature.
    index: Vec<Option<u8>>,
}

impl GpuSensorsSource {
    pub fn new() -> Result<Self, SourceError> {
        let opened = open_adapters();
        if opened.is_empty() {
            return Err(unavailable());
        }
        let known: Vec<u64> = crate::hardware::gpu::adapters()
            .into_iter()
            .map(|(_, luid)| luid)
            .collect();
        Ok(Self {
            index: opened
                .iter()
                .map(|(_, luid)| {
                    let i = known.iter().position(|l| l == luid)?;
                    u8::try_from(i).ok()
                })
                .collect(),
            adapters: opened.into_iter().map(|(handle, _)| handle).collect(),
        })
    }
}

/// Each adapter's handle and LUID.
fn open_adapters() -> Vec<(u32, u64)> {
    unsafe {
        let mut e = D3DKMT_ENUMADAPTERS2::default();
        if D3DKMTEnumAdapters2(&mut e).is_err() || e.NumAdapters == 0 {
            return Vec::new();
        }
        let mut infos = vec![D3DKMT_ADAPTERINFO::default(); e.NumAdapters as usize];
        e.pAdapters = infos.as_mut_ptr();
        if D3DKMTEnumAdapters2(&mut e).is_err() {
            return Vec::new();
        }
        infos[..e.NumAdapters as usize]
            .iter()
            .map(|a| (a.hAdapter, crate::hardware::gpu::luid_key(a.AdapterLuid)))
            .collect()
    }
}

fn query(adapter: u32) -> Option<SensorReading> {
    let mut perf = D3DKMT_ADAPTER_PERFDATA::default();
    let mut q = D3DKMT_QUERYADAPTERINFO {
        hAdapter: adapter,
        Type: KMTQAITYPE_ADAPTERPERFDATA,
        pPrivateDriverData: (&mut perf as *mut D3DKMT_ADAPTER_PERFDATA).cast(),
        PrivateDriverDataSize: size_of::<D3DKMT_ADAPTER_PERFDATA>() as u32,
    };
    unsafe { D3DKMTQueryAdapterInfo(&mut q) }
        .is_ok()
        .then_some(SensorReading {
            temp_deci_c: perf.Temperature,
            fan_rpm: perf.FanRPM,
            power_deci_percent: perf.Power,
            mem_clock_hz: perf.MemoryFrequency,
        })
}

impl Source for GpuSensorsSource {
    fn id(&self) -> SourceId {
        SourceId::GpuSensors
    }

    fn sample(&mut self, out: &mut Snapshot) -> Result<(), SourceError> {
        let mut readings = Vec::with_capacity(self.adapters.len());
        for (&adapter, &index) in self.adapters.iter().zip(&self.index) {
            let Some(reading) = query(adapter) else {
                continue;
            };
            if let Some(i) = index
                && reading.temp_deci_c > 0
            {
                out.set(
                    MetricKey::GpuAdapterTempC(i),
                    f64::from(reading.temp_deci_c) / 10.0,
                );
            }
            readings.push(reading);
        }
        let reading = pick_reading(&readings).ok_or_else(unavailable)?;
        publish(&reading, out);
        Ok(())
    }
}

impl Drop for GpuSensorsSource {
    fn drop(&mut self) {
        for &adapter in &self.adapters {
            let close = D3DKMT_CLOSEADAPTER { hAdapter: adapter };
            unsafe {
                let _ = D3DKMTCloseAdapter(&close);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(temp: u32, fan: u32, power: u32) -> SensorReading {
        SensorReading {
            temp_deci_c: temp,
            fan_rpm: fan,
            power_deci_percent: power,
            mem_clock_hz: 0,
        }
    }

    #[test]
    fn the_hottest_adapter_with_a_temperature_is_reported() {
        assert_eq!(
            pick_reading(&[r(0, 0, 0), r(491, 0, 106), r(620, 1500, 300)]),
            Some(r(620, 1500, 300))
        );
    }

    #[test]
    fn no_adapter_with_a_temperature_is_unavailable() {
        assert_eq!(pick_reading(&[r(0, 0, 0)]), None);
        assert_eq!(pick_reading(&[]), None);
    }

    #[test]
    fn readings_convert_to_metrics() {
        let mut s = Snapshot::default();
        publish(
            &SensorReading {
                temp_deci_c: 491,
                fan_rpm: 1200,
                power_deci_percent: 106,
                mem_clock_hz: 7_001_000_000,
            },
            &mut s,
        );
        assert_eq!(s.get(MetricKey::GpuTempC), Some(49.1));
        assert_eq!(s.get(MetricKey::GpuFanRpm), Some(1200.0));
        assert!((s.get(MetricKey::GpuPowerPercent).unwrap() - 10.6).abs() < 1e-9);
        assert_eq!(s.get(MetricKey::GpuMemClockMhz), Some(7001.0));
    }

    #[test]
    fn live_sensors_report_or_are_cleanly_unavailable() {
        match GpuSensorsSource::new() {
            Ok(mut source) => {
                let mut s = Snapshot::default();
                match source.sample(&mut s) {
                    Ok(()) => {
                        assert!((0.0..150.0).contains(&s.get(MetricKey::GpuTempC).unwrap()))
                    }
                    Err(e) => assert_eq!(e.to_string(), "GPU temperature unavailable"),
                }
            }
            Err(e) => assert_eq!(e.to_string(), "GPU temperature unavailable"),
        }
    }
}
