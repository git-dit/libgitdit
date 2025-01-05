// git-dit - the distributed issue tracker for git
// Copyright (C) 2025 Julian Ganz <neither@nut.email>
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.
//! Base type and trait definitions

use std::fmt;

use crate::error;

/// Base types
///
/// This trait defines some base types of underlying git implementations.
pub trait Base {
    /// Type used for representing Object IDs
    type Oid: Clone + fmt::Debug + fmt::Display;

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
