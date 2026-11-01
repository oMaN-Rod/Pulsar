//! GPU temperature, fan and power from the display driver, as Task Manager
//! reads them: no admin rights, no vendor libraries.

use std::time::{Duration, Instant};

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

/// How often the handles are reopened, to pick up adapters that came since.
const REOPEN: Duration = Duration::from_secs(30);
/// How soon after one failed or none were found.
const REOPEN_AFTER_FAILURE: Duration = Duration::from_secs(5);

/// The driver's sensors, apart from the source so tests can fake them.
trait AdapterSensors: Send {
    /// Each adapter's handle and LUID.
    fn open(&mut self) -> Vec<(u32, u64)>;
    fn query(&self, adapter: u32) -> Option<SensorReading>;
    fn close(&mut self, adapter: u32);
}

/// Holds the adapter handles open, closing them on drop.
pub struct GpuSensorsSource {
    sensors: Box<dyn AdapterSensors>,
    adapters: Vec<(u32, u64)>,
    opened: Option<Instant>,
    failed: bool,
}

impl Default for GpuSensorsSource {
    fn default() -> Self {
        Self::with_sensors(Box::new(DriverSensors))
    }
}

impl GpuSensorsSource {
    fn with_sensors(sensors: Box<dyn AdapterSensors>) -> Self {
        Self {
            sensors,
            adapters: Vec::new(),
            opened: None,
            failed: false,
        }
    }

    fn close_all(&mut self) {
        for (adapter, _) in self.adapters.drain(..) {
            self.sensors.close(adapter);
        }
    }

    fn reopen_if_due(&mut self, now: Instant) {
        let wait = if self.failed || self.adapters.is_empty() {
            REOPEN_AFTER_FAILURE
        } else {
            REOPEN
        };
        if self.opened.is_none_or(|t| now.duration_since(t) >= wait) {
            self.close_all();
            self.adapters = self.sensors.open();
            self.opened = Some(now);
        }
    }

    fn sample_at(&mut self, now: Instant, out: &mut Snapshot) -> Result<(), SourceError> {
        self.reopen_if_due(now);
        let mut readings = Vec::with_capacity(self.adapters.len());
        self.failed = false;
        for &(adapter, luid) in &self.adapters {
            let Some(reading) = self.sensors.query(adapter) else {
                self.failed = true;
                continue;
            };
            out.gpu_readings.seen.insert(luid);
            if reading.temp_deci_c > 0 {
                let temp_c = f64::from(reading.temp_deci_c) / 10.0;
                out.gpu_readings.temp_c.insert(luid, temp_c);
            }
            readings.push(reading);
        }
        let reading = pick_reading(&readings).ok_or_else(unavailable)?;
        publish(&reading, out);
        Ok(())
    }
}

struct DriverSensors;

impl AdapterSensors for DriverSensors {
    fn open(&mut self) -> Vec<(u32, u64)> {
        open_adapters()
    }

    fn query(&self, adapter: u32) -> Option<SensorReading> {
        query(adapter)
    }

    fn close(&mut self, adapter: u32) {
        let close = D3DKMT_CLOSEADAPTER { hAdapter: adapter };
        unsafe {
            let _ = D3DKMTCloseAdapter(&close);
        }
    }
}

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
        self.sample_at(Instant::now(), out)
    }
}

impl Drop for GpuSensorsSource {
    fn drop(&mut self) {
        self.close_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    /// Handles are the LUID times ten.
    #[derive(Default)]
    struct Fake {
        temps: Vec<(u64, u32)>,
        opens: usize,
        open_handles: Vec<u32>,
    }

    #[derive(Clone, Default)]
    struct FakeSensors(Arc<Mutex<Fake>>);

    impl FakeSensors {
        fn set(&self, temps: &[(u64, u32)]) {
            self.0.lock().unwrap().temps = temps.to_vec();
        }
    }

    impl AdapterSensors for FakeSensors {
        fn open(&mut self) -> Vec<(u32, u64)> {
            let mut fake = self.0.lock().unwrap();
            fake.opens += 1;
            let handles: Vec<(u32, u64)> = fake
                .temps
                .iter()
                .map(|&(luid, _)| (luid as u32 * 10, luid))
                .collect();
            fake.open_handles = handles.iter().map(|&(h, _)| h).collect();
            handles
        }

        fn query(&self, adapter: u32) -> Option<SensorReading> {
            let fake = self.0.lock().unwrap();
            let &(_, temp) = fake
                .temps
                .iter()
                .find(|&&(luid, _)| luid as u32 * 10 == adapter)?;
            Some(r(temp, 0, 0))
        }

        fn close(&mut self, adapter: u32) {
            self.0
                .lock()
                .unwrap()
                .open_handles
                .retain(|&h| h != adapter);
        }
    }

    fn sampled(source: &mut GpuSensorsSource, now: Instant) -> Vec<(u64, f64)> {
        let mut s = Snapshot::default();
        let _ = source.sample_at(now, &mut s);
        let mut temps: Vec<_> = s.gpu_readings.temp_c.into_iter().collect();
        temps.sort_by_key(|&(luid, _)| luid);
        temps
    }

    #[test]
    fn a_card_that_arrives_is_read_after_the_refresh() {
        let fake = FakeSensors::default();
        fake.set(&[(1, 500)]);
        let mut source = GpuSensorsSource::with_sensors(Box::new(fake.clone()));
        let start = Instant::now();
        assert_eq!(sampled(&mut source, start), [(1, 50.0)]);

        fake.set(&[(2, 600), (1, 510)]);
        assert_eq!(
            sampled(&mut source, start + Duration::from_secs(1)),
            [(1, 51.0)],
            "the open handles still read their own card"
        );
        assert_eq!(sampled(&mut source, start + REOPEN), [(1, 51.0), (2, 60.0)]);
        assert_eq!(fake.0.lock().unwrap().opens, 2);
    }

    #[test]
    fn a_card_that_goes_loses_its_reading_and_the_handles_are_reopened_soon() {
        let fake = FakeSensors::default();
        fake.set(&[(1, 500), (2, 600)]);
        let mut source = GpuSensorsSource::with_sensors(Box::new(fake.clone()));
        let start = Instant::now();
        sampled(&mut source, start);

        fake.set(&[(2, 610)]);
        assert_eq!(
            sampled(&mut source, start + Duration::from_secs(1)),
            [(2, 61.0)]
        );
        assert_eq!(fake.0.lock().unwrap().opens, 1);
        assert_eq!(
            sampled(&mut source, start + REOPEN_AFTER_FAILURE),
            [(2, 61.0)]
        );
        let fake = fake.0.lock().unwrap();
        assert_eq!(fake.opens, 2);
        assert_eq!(fake.open_handles, [20], "the old handles are closed");
    }

    #[test]
    fn a_source_started_with_no_adapters_finds_them_later() {
        let fake = FakeSensors::default();
        let mut source = GpuSensorsSource::with_sensors(Box::new(fake.clone()));
        let start = Instant::now();
        let mut s = Snapshot::default();
        assert!(source.sample_at(start, &mut s).is_err());

        fake.set(&[(1, 500)]);
        assert_eq!(
            sampled(&mut source, start + REOPEN_AFTER_FAILURE),
            [(1, 50.0)]
        );
    }

    #[test]
    fn an_adapter_without_a_sensor_is_seen_without_a_temperature() {
        let fake = FakeSensors::default();
        fake.set(&[(1, 0), (2, 600)]);
        let mut source = GpuSensorsSource::with_sensors(Box::new(fake));
        let mut s = Snapshot::default();
        source.sample_at(Instant::now(), &mut s).unwrap();
        assert!(s.gpu_readings.seen.contains(&1));
        assert!(!s.gpu_readings.temp_c.contains_key(&1));
    }

    #[test]
    fn dropping_the_source_closes_the_handles() {
        let fake = FakeSensors::default();
        fake.set(&[(1, 500)]);
        let mut source = GpuSensorsSource::with_sensors(Box::new(fake.clone()));
        sampled(&mut source, Instant::now());
        drop(source);
        assert!(fake.0.lock().unwrap().open_handles.is_empty());
    }

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
        let mut s = Snapshot::default();
        match GpuSensorsSource::default().sample(&mut s) {
            Ok(()) => assert!((0.0..150.0).contains(&s.get(MetricKey::GpuTempC).unwrap())),
            Err(e) => assert_eq!(e.to_string(), "GPU temperature unavailable"),
        }
    }
}
