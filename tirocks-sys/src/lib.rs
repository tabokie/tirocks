// Copyright 2022 TiKV Project Authors. Licensed under Apache-2.0.

#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]

// bzip2 requires a user-defined `bz_internal_error` hook to handle errors
// as it's an alloc free library. bzip2_sys provides a default implementation.
extern crate bzip2_sys;

#[allow(clippy::all)]
mod bindings {
    include!(env!("BINDING_PATH"));
}

use std::{ffi::CStr, str::Utf8Error};

pub use bindings::*;

/// Convert a rust slice to rocksdb slice.
///
/// # Safety
///
/// Caller should guarantee the data lives as long as the returned slice.
// Because the layout of rocksdb_Slice is exact the same as &[u8], this function
// will be optimized away by compiler.
#[inline]
pub unsafe fn r(src: &[u8]) -> rocksdb_Slice {
    rocksdb_Slice {
        data_: src.as_ptr() as _,
        size_: src.len(),
    }
}

/// Convert a rocksdb slice to rust slice.
///
/// # Safety
///
/// Caller should guarantee the data lives as long as the returned slice.
// Because the layout of rocksdb_Slice is exact the same as &[u8], this function
// will be optimized away by compiler.
#[inline]
pub unsafe fn s<'a>(src: rocksdb_Slice) -> &'a [u8] {
    std::slice::from_raw_parts(src.data_ as _, src.size_)
}

impl rocksdb_Status {
    #[inline]
    pub fn with_code(code: rocksdb_Status_Code) -> rocksdb_Status {
        rocksdb_Status {
            code_: code,
            subcode_: rocksdb_Status_SubCode::kNone,
            sev_: rocksdb_Status_Severity::kNoError,
            state_: std::ptr::null(),
        }
    }

    /// Build a status with given errors.
    ///
    /// # Panics
    ///
    /// `code` should not be `kOK`.
    #[inline]
    pub fn with_error(code: rocksdb_Status_Code, state: impl AsRef<[u8]>) -> rocksdb_Status {
        assert_ne!(code, rocksdb_Status_Code::kOk);
        let state = state.as_ref();
        let state_ = if !state.is_empty() {
            unsafe { crocksdb_to_cplus_array(r(state)) }
        } else {
            std::ptr::null()
        };
        rocksdb_Status {
            code_: code,
            subcode_: rocksdb_Status_SubCode::kNone,
            sev_: rocksdb_Status_Severity::kNoError,
            state_,
        }
    }

    #[inline]
    fn clear_state(&mut self) {
        if self.state_.is_null() {
            return;
        }
        unsafe {
            crocksdb_free_cplus_array(self.state_);
            self.state_ = std::ptr::null();
        }
    }

    #[inline]
    pub fn ok(&self) -> bool {
        self.code_ == rocksdb_Status_Code::kOk
    }

    #[inline]
    pub fn state(&self) -> Option<&[u8]> {
        if self.state_.is_null() {
            None
        } else {
            unsafe { Some(CStr::from_ptr(self.state_).to_bytes()) }
        }
    }

    #[inline]
    pub fn message(&self) -> Result<Option<&str>, Utf8Error> {
        if self.state_.is_null() {
            Ok(None)
        } else {
            unsafe { CStr::from_ptr(self.state_).to_str().map(Some) }
        }
    }

    #[inline]
    pub fn code(&self) -> rocksdb_Status_Code {
        self.code_
    }

    #[inline]
    pub fn severity(&self) -> rocksdb_Status_Severity {
        self.sev_
    }

    #[inline]
    pub fn set_severity(&mut self, severity: rocksdb_Status_Severity) {
        self.sev_ = severity;
    }

    #[inline]
    pub fn sub_code(&self) -> rocksdb_Status_SubCode {
        self.subcode_
    }

    #[inline]
    pub fn set_sub_code(&mut self, sub_code: rocksdb_Status_SubCode) {
        self.subcode_ = sub_code;
    }
}

impl Drop for rocksdb_Status {
    #[inline]
    fn drop(&mut self) {
        self.clear_state();
    }
}

#[macro_export]
macro_rules! ffi_try {
    ($func:ident($($arg:expr),+)) => ({
        let mut status = $crate::rocksdb_Status::with_code($crate::rocksdb_Status_Code::kOk);
        let res = $crate::$func($($arg),+, &mut status);
        if status.ok() {
            res
        } else {
            return Err(status.into());
        }
    });
    ($func:ident()) => ({
        let mut status = $crate::rocksdb_Status::with_code($crate::rocksdb_Status_Code::kOk);
        let res = $crate::$func(&mut status);
        if status.ok() {
            res
        } else {
            return Err(status.into());
        }
    })
}

#[cfg(test)]
mod tests {
    use crate::{
        crocksdb_close, crocksdb_options_create, crocksdb_options_destroy,
        crocksdb_options_set_create_if_missing, r, rocksdb_Status_Code,
    };

    use super::s;

    #[test]
    fn test_smoke() {
        assert_eq!(b"rocksdb.cfstats-no-file-histogram", unsafe {
            s(super::crocksdb_property_name_cf_stats_no_file_histogram)
        });
    }

    #[test]
    fn test_status() {
        let td = tempfile::tempdir().unwrap();
        let path = td.path().to_str().unwrap();
        unsafe {
            let opt = crocksdb_options_create();
            crocksdb_options_set_create_if_missing(opt, 0);
            let s: Result<_, super::rocksdb_Status> =
                (|| Ok(ffi_try!(crocksdb_open(opt, r(path.as_bytes())))))();
            let e = s.unwrap_err();
            assert!(!e.ok());
            assert_eq!(e.code_, rocksdb_Status_Code::kInvalidArgument);
            let msg = e.message().unwrap().unwrap();
            assert!(msg.contains("does not exist"), "{}", msg);

            crocksdb_options_set_create_if_missing(opt, 1);
            let s: Result<_, super::rocksdb_Status> =
                (|| Ok(ffi_try!(crocksdb_open(opt, r(path.as_bytes())))))();
            let e = s.unwrap();
            crocksdb_close(e);
            crocksdb_options_destroy(opt);
        }
    }
}