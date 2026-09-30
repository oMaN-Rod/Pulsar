//! Installed font families, for the font picker.

use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FACTORY_TYPE_SHARED, DWriteCreateFactory, IDWriteFactory, IDWriteFontCollection,
    IDWriteLocalizedStrings,
};
use windows::core::{BOOL, w};

fn english_name(names: &IDWriteLocalizedStrings) -> Option<String> {
    unsafe {
        let (mut index, mut exists) = (0u32, BOOL(0));
        names
            .FindLocaleName(w!("en-us"), &mut index, &mut exists)
            .ok()?;
        if !exists.as_bool() {
            index = 0;
        }
        let len = names.GetStringLength(index).ok()? as usize;
        let mut buf = vec![0u16; len + 1];
        names.GetString(index, &mut buf).ok()?;
        Some(String::from_utf16_lossy(&buf[..len]))
    }
}

/// Family names of the system font collection, sorted case-insensitively.
pub fn system_families() -> Vec<String> {
    let mut families = Vec::new();
    unsafe {
        let Ok(factory) = DWriteCreateFactory::<IDWriteFactory>(DWRITE_FACTORY_TYPE_SHARED) else {
            return families;
        };
        let mut collection: Option<IDWriteFontCollection> = None;
        if factory
            .GetSystemFontCollection(&mut collection, false)
            .is_err()
        {
            return families;
        }
        let Some(collection) = collection else {
            return families;
        };
        for i in 0..collection.GetFontFamilyCount() {
            if let Some(name) = collection
                .GetFontFamily(i)
                .and_then(|f| f.GetFamilyNames())
                .ok()
                .and_then(|names| english_name(&names))
            {
                families.push(name);
            }
        }
    }
    families.sort_by_key(|f| f.to_lowercase());
    families.dedup();
    families
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_families_include_segoe_ui_and_are_sorted() {
        let families = system_families();
        assert!(families.iter().any(|f| f == "Segoe UI"));
        let mut sorted = families.clone();
        sorted.sort_by_key(|f| f.to_lowercase());
        assert_eq!(families, sorted);
    }
}
