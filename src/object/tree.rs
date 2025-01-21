// git-dit - the distributed issue tracker for git
// Copyright (C) 2025 Julian Ganz <neither@nut.email>
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.
//! Tree related facilities

/// A builder for trees
pub trait Builder {
    /// Type used for representing Object IDs
    type Oid;

    /// Error type associated with this entity
    type Error;

    /// Write the tree to the object database
    fn write(self) -> Result<Self::Oid, Self::Error>;
}

impl Builder for git2::TreeBuilder<'_> {
    type Oid = git2::Oid;
    type Error = git2::Error;

    fn write(self) -> Result<Self::Oid, Self::Error> {
        git2::TreeBuilder::write(&self)
    }
}
