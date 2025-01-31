// git-dit - the distributed issue tracker for git
// Copyright (C) 2025 Julian Ganz <neither@nut.email>
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.
//! Base type and trait definitions

use std::fmt;
use std::hash::Hash;

use crate::error;

/// Base types
///
/// This trait defines some base types of underlying git implementations.
pub trait Base {
    /// Type used for representing Object IDs
    type Oid: Clone + fmt::Debug + fmt::Display + Eq + Hash;

    /// Type used for representing references
    type Reference<'a>;

    /// (Inner) error type associated with this entity
    type InnerError: for<'a> error::InnerError<Oid = Self::Oid, Reference<'a> = Self::Reference<'a>>;
}

impl Base for git2::Repository {
    type Oid = git2::Oid;
    type Reference<'a> = git2::Reference<'a>;
    type InnerError = git2::Error;
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[derive(Copy, Clone, Default, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct TestOid([u8; 20]);

    impl std::ops::AddAssign<u8> for TestOid {
        fn add_assign(&mut self, rhs: u8) {
            self.0.iter_mut().rev().fold(rhs, |c, n| {
                let [h, l] = (c as u16 + *n as u16).to_be_bytes();
                *n = l;
                h
            });
        }
    }

    impl fmt::Display for TestOid {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            self.0.into_iter().try_for_each(|c| write!(f, "{c:02x}"))
        }
    }

    impl std::str::FromStr for TestOid {
        type Err = String;

        fn from_str(mut s: &str) -> Result<Self, Self::Err> {
            let mut res: [u8; 20] = Default::default();
            for i in 0..20 {
                let (part, rest) = s.split_at(2);
                res[i] = u8::from_str_radix(part, 16).map_err(|_| format!("Not hex: {part}"))?;
                s = rest;
            }
            Ok(TestOid(res))
        }
    }

    impl PartialEq<&str> for TestOid {
        fn eq(&self, other: &&str) -> bool {
            other.parse().map(|i: Self| self == &i).unwrap_or(false)
        }
    }

    #[test]
    fn test_oid_incremental_order() {
        let mut id = TestOid::default();
        id += 1; // id = [1, 0, 0, ...]
        let one = id.clone();
        id += 255; // id = [0, 1, 0, ...]
        assert!(one < id);
    }
}
