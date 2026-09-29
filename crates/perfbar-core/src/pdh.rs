use windows::Win32::System::Performance::{
    PDH_CSTATUS_NEW_DATA, PDH_CSTATUS_VALID_DATA, PDH_FMT, PDH_FMT_COUNTERVALUE,
    PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE, PDH_HCOUNTER, PDH_HQUERY, PDH_MORE_DATA,
    PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW,
    PdhGetFormattedCounterValue, PdhOpenQueryW,
};
use windows::core::{HSTRING, PCWSTR};

const PDH_FMT_NOCAP100: u32 = 0x0000_8000;
const FMT: PDH_FMT = PDH_FMT(PDH_FMT_DOUBLE.0 | PDH_FMT_NOCAP100);

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("PDH error 0x{0:08X}")]
pub struct PdhError(pub u32);

fn check(status: u32) -> Result<(), PdhError> {
    if status == 0 {
        Ok(())
    } else {
        Err(PdhError(status))
    }
}

fn valid(cstatus: u32) -> bool {
    cstatus == PDH_CSTATUS_VALID_DATA || cstatus == PDH_CSTATUS_NEW_DATA
}

/// A PDH query. Counters added to it are closed when the query is dropped,
/// so a `Counter` must not outlive the `Query` that created it.
pub struct Query(PDH_HQUERY);

// PDH handles may be used from any thread; each Query is owned by one source.
unsafe impl Send for Query {}

impl Query {
    pub fn new() -> Result<Self, PdhError> {
        let mut handle = PDH_HQUERY::default();
        check(unsafe { PdhOpenQueryW(PCWSTR::null(), 0, &mut handle) })?;
        Ok(Self(handle))
    }

    /// Adds a counter by its English path, e.g. `\Processor Information(_Total)\% Processor Utility`.
    pub fn add(&self, path: &str) -> Result<Counter, PdhError> {
        let mut counter = PDH_HCOUNTER::default();
        let path = HSTRING::from(path);
        check(unsafe { PdhAddEnglishCounterW(self.0, &path, 0, &mut counter) })?;
        Ok(Counter(counter))
    }

    pub fn collect(&self) -> Result<(), PdhError> {
        check(unsafe { PdhCollectQueryData(self.0) })
    }
}

impl Drop for Query {
    fn drop(&mut self) {
        unsafe { PdhCloseQuery(self.0) };
    }
}

pub struct Counter(PDH_HCOUNTER);

unsafe impl Send for Counter {}

impl Counter {
    pub fn value(&self) -> Result<f64, PdhError> {
        let mut value = PDH_FMT_COUNTERVALUE::default();
        check(unsafe { PdhGetFormattedCounterValue(self.0, FMT, None, &mut value) })?;
        if !valid(value.CStatus) {
            return Err(PdhError(value.CStatus));
        }
        Ok(unsafe { value.Anonymous.doubleValue })
    }

    /// Returns `(instance name, value)` for every instance of a wildcard counter.
    /// Instances whose data is not valid yet are skipped.
    pub fn array(&self) -> Result<Vec<(String, f64)>, PdhError> {
        let mut bytes = 0u32;
        let mut count = 0u32;
        let status =
            unsafe { PdhGetFormattedCounterArrayW(self.0, FMT, &mut bytes, &mut count, None) };
        if status != PDH_MORE_DATA {
            check(status)?;
            return Ok(Vec::new());
        }

        let item_size = size_of::<PDH_FMT_COUNTERVALUE_ITEM_W>();
        let mut buffer =
            vec![PDH_FMT_COUNTERVALUE_ITEM_W::default(); (bytes as usize).div_ceil(item_size)];
        check(unsafe {
            PdhGetFormattedCounterArrayW(
                self.0,
                FMT,
                &mut bytes,
                &mut count,
                Some(buffer.as_mut_ptr()),
            )
        })?;

        let items = buffer
            .iter()
            .take(count as usize)
            .filter(|item| valid(item.FmtValue.CStatus))
            .map(|item| {
                let name = unsafe { item.szName.to_string() }.unwrap_or_default();
                (name, unsafe { item.FmtValue.Anonymous.doubleValue })
            })
            .collect();
        Ok(items)
    }
}
