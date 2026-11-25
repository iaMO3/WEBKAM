use super::Source_Impl;
use std::{ffi::c_void, ptr};
use windows::{
    Win32::{
        Media::MediaFoundation::{
            IMFAttributes, IMFAttributes_Impl, MF_ATTRIBUTE_TYPE, MF_ATTRIBUTES_MATCH_TYPE,
        },
        System::Com::StructuredStorage::PROPVARIANT,
    },
    core::{BOOL, GUID, IUnknown, Interface, PCWSTR, PWSTR, Ref, Result},
};

// The source and its activation interface expose the same Media Foundation store.
macro_rules! forward {
    ($method:ident ( $($arg:ident : $argtype:ty),* )) => {
        fn $method(&self, $($arg: $argtype),*) -> Result<()> {
            let store = &self.attributes;
            unsafe { (store.vtable().$method)(store.as_raw(), $($arg),*).ok() }
        }
    };
    ($method:ident ( $($arg:ident : $argtype:ty),* ) -> $output:ty) => {
        fn $method(&self, $($arg: $argtype),*) -> Result<$output> {
            let store = &self.attributes;
            let mut output = <$output>::default();
            unsafe { (store.vtable().$method)(store.as_raw(), $($arg,)* &mut output).map(|| output) }
        }
    };
}

impl IMFAttributes_Impl for Source_Impl {
    forward!(GetItem(key: *const GUID, value: *mut PROPVARIANT));
    forward!(GetItemType(key: *const GUID) -> MF_ATTRIBUTE_TYPE);
    forward!(CompareItem(key: *const GUID, value: *const PROPVARIANT) -> BOOL);
    forward!(GetUINT32(key: *const GUID) -> u32);
    forward!(GetUINT64(key: *const GUID) -> u64);
    forward!(GetDouble(key: *const GUID) -> f64);
    forward!(GetGUID(key: *const GUID) -> GUID);
    forward!(GetStringLength(key: *const GUID) -> u32);
    forward!(GetString(key: *const GUID, value: PWSTR, capacity: u32, length: *mut u32));
    forward!(GetAllocatedString(key: *const GUID, value: *mut PWSTR, length: *mut u32));
    forward!(GetBlobSize(key: *const GUID) -> u32);
    forward!(GetBlob(key: *const GUID, buffer: *mut u8, capacity: u32, length: *mut u32));
    forward!(GetAllocatedBlob(key: *const GUID, buffer: *mut *mut u8, length: *mut u32));
    forward!(GetUnknown(key: *const GUID, iid: *const GUID, value: *mut *mut c_void));
    forward!(SetItem(key: *const GUID, value: *const PROPVARIANT));
    forward!(DeleteItem(key: *const GUID));
    forward!(DeleteAllItems());
    forward!(SetUINT32(key: *const GUID, value: u32));
    forward!(SetUINT64(key: *const GUID, value: u64));
    forward!(SetDouble(key: *const GUID, value: f64));
    forward!(SetGUID(key: *const GUID, value: *const GUID));
    forward!(SetBlob(key: *const GUID, buffer: *const u8, length: u32));
    forward!(LockStore());
    forward!(UnlockStore());
    forward!(GetCount() -> u32);
    forward!(GetItemByIndex(index: u32, key: *mut GUID, value: *mut PROPVARIANT));

    fn Compare(&self, other: Ref<IMFAttributes>, how: MF_ATTRIBUTES_MATCH_TYPE) -> Result<BOOL> {
        let store = &self.attributes;
        let other = other.as_ref().map_or(ptr::null_mut(), Interface::as_raw);
        let mut equal = BOOL::default();
        unsafe { (store.vtable().Compare)(store.as_raw(), other, how, &mut equal).map(|| equal) }
    }
    fn SetString(&self, key: *const GUID, value: &PCWSTR) -> Result<()> {
        unsafe { self.attributes.SetString(key, *value) }
    }
    fn SetUnknown(&self, key: *const GUID, value: Ref<IUnknown>) -> Result<()> {
        unsafe { self.attributes.SetUnknown(key, value.as_ref()) }
    }
    fn CopyAllItems(&self, destination: Ref<IMFAttributes>) -> Result<()> {
        unsafe { self.attributes.CopyAllItems(destination.as_ref()) }
    }
}
