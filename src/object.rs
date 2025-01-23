// git-dit - the distributed issue tracker for git
// Copyright (C) 2025 Julian Ganz <neither@nut.email>
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.
//! Object related facilities

use crate::base::Base;
use crate::error::{self, ResultExt};

pub mod commit;
pub mod tree;

use self::commit::Commit;

/// An object database
pub trait Database<'r>: Base {
    /// Type used for representing commits
    type Commit: Commit<Oid = Self::Oid>;

    /// Type used for representing trees
    type Tree;

    /// Type for representing signautres
    type Signature<'s>;

    /// Retrieve the signature to use for author
    fn author(&self) -> error::Result<Self::Signature<'_>, Self::InnerError>;

    /// Retrieve the signature to use for committer
    fn committer(&self) -> error::Result<Self::Signature<'_>, Self::InnerError>;

    /// Retrieve a specific commit
    fn find_commit(&'r self, oid: Self::Oid) -> error::Result<Self::Commit, Self::InnerError>;

    /// Retrieve a specific tree
    fn find_tree(&'r self, oid: Self::Oid) -> error::Result<Self::Tree, Self::InnerError>;
}

impl<'r> Database<'r> for git2::Repository {
    type Commit = git2::Commit<'r>;
    type Tree = git2::Tree<'r>;
    type Signature<'s> = git2::Signature<'s>;

    fn author(&self) -> error::Result<Self::Signature<'_>, Self::InnerError> {
        self.signature()
            .wrap_with_kind(error::Kind::CannotGetSignature)
    }

    fn committer(&self) -> error::Result<Self::Signature<'_>, Self::InnerError> {
        self.author()
    }

    fn find_commit(&'r self, oid: Self::Oid) -> error::Result<Self::Commit, Self::InnerError> {
        git2::Repository::find_commit(self, oid).wrap_with_kind(error::Kind::CannotGetCommit)
    }

    fn find_tree(&'r self, oid: Self::Oid) -> error::Result<Self::Tree, Self::InnerError> {
        git2::Repository::find_tree(self, oid).wrap_with_kind(error::Kind::CannotGetTree)
    }
}
