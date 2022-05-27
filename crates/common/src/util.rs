use hdk::prelude::*;

/// Tries to do a DHT get to retrieve data for the entry_hash,
/// and if this get is successful and we get some element, tries
/// to convert this element into a type T and return the result
pub fn try_get_and_convert<T: TryFrom<Entry>>(
    entry_hash: EntryHash,
    get_options: GetOptions,
) -> ExternResult<T> {
    match get(entry_hash.clone(), get_options)? {
        Some(element) => try_from_element(element),
        None => Err(WasmError::Guest(format!(
            "There is no element at the hash {}",
            entry_hash
        ))),
    }
}

pub fn try_get_and_convert_with_hh<T: TryFrom<Entry>>(
    entry_hash: EntryHash,
    get_options: GetOptions,
) -> ExternResult<(T, HeaderHash)> {
    match get(entry_hash.clone(), get_options)? {
        Some(element) => {
            let hh = element.header_address().clone();
            let v = try_from_element(element)?;
            Ok((v, hh))
        }
        None => Err(WasmError::Guest(format!(
            "There is no element at the hash {}",
            entry_hash
        ))),
    }
}

pub fn get_hh(entry_hash: EntryHash, get_options: GetOptions) -> ExternResult<HeaderHash> {
    match get(entry_hash.clone(), get_options)? {
        Some(element) => {
            let hh = element.header_address().clone();
            Ok(hh)
        }
        None => Err(WasmError::Guest(format!(
            "There is no element at the hash {}",
            entry_hash
        ))),
    }
}

/// Attempts to get an element at the entry_hash and returns it
/// if the element exists
#[allow(dead_code)]
pub fn try_get_element(entry_hash: EntryHash, get_options: GetOptions) -> ExternResult<Element> {
    match get(entry_hash.clone(), get_options)? {
        Some(element) => Ok(element),
        None => Err(WasmError::Guest(format!(
            "There is no element at the hash {}",
            entry_hash
        ))),
    }
}

/// Tries to extract the entry from the element, and if the entry is there
/// tries to convert it to type T and return the result
#[allow(dead_code)]
pub fn try_from_element<T: TryFrom<Entry>>(element: Element) -> ExternResult<T> {
    match element.entry() {
        element::ElementEntry::Present(entry) => T::try_from(entry.clone()).map_err(|_| {
            WasmError::Guest(format!(
                "Couldn't convert Element entry {:?} into data type {}",
                entry,
                std::any::type_name::<T>()
            ))
        }),
        _ => Err(WasmError::Guest(format!(
            "Element {:?} does not have an entry",
            element
        ))),
    }
}
