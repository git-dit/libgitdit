// git-dit - the distributed issue tracker for git
// Copyright (C) 2025 Julian Ganz <neither@nut.email>
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.
//! Commit related facilities

/// A git commit
pub trait Commit {
    /// Type used for representing Object IDs
    type Oid;

    /// Type used for git signatures
    type Signature<'s>
    where
        Self: 's;

    /// Retrieve the commit id
    fn id(&self) -> Self::Oid;

    /// Retrieve the author of the commit
    fn author(&self) -> Self::Signature<'_>;

    /// Retrieve the commit of the commit
    fn committer(&self) -> Self::Signature<'_>;

    /// Retrieve the full commit message
    ///
    /// The full commit message includes the subject, body and trailers.
    fn message(&self) -> Result<&str, std::str::Utf8Error>;

    /// Retrieve the ids of this commit's parents
    fn parent_ids(&self) -> impl IntoIterator<Item = Self::Oid> + '_;

    /// Retrieve this commit's tree's id
    fn tree_id(&self) -> Self::Oid;
}

impl Commit for git2::Commit<'_> {
    type Oid = git2::Oid;

    type Signature<'s>
        = git2::Signature<'s>
    where
        Self: 's;

    fn id(&self) -> Self::Oid {
        git2::Commit::id(self)
    }

    fn author(&self) -> Self::Signature<'_> {
        git2::Commit::author(self)
    }

    fn committer(&self) -> Self::Signature<'_> {
        git2::Commit::committer(self)
    }

    fn message(&self) -> Result<&str, std::str::Utf8Error> {
        std::str::from_utf8(git2::Commit::message_bytes(self))
    }

    fn parent_ids(&self) -> impl IntoIterator<Item = Self::Oid> + '_ {
        git2::Commit::parent_ids(self)
    }

    fn tree_id(&self) -> Self::Oid {
        git2::Commit::tree_id(self)
    }
}
