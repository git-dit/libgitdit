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

use commit::Commit;

/// An object database
pub trait Database<'r>: Base {
    /// Type used for representing commits
    type Commit: Commit<Oid = Self::Oid>;

    /// Type used for representing trees
    type Tree;

    /// Type for representing signautres
    type Signature<'s>;

    /// A builder for trees
    type TreeBuilder: tree::Builder<Oid = Self::Oid, Error: Into<Self::InnerError>>;

    /// Retrieve the signature to use for author
    fn author(&self) -> error::Result<Self::Signature<'_>, Self::InnerError>;

    /// Retrieve the signature to use for committer
    fn committer(&self) -> error::Result<Self::Signature<'_>, Self::InnerError>;

    /// Retrieve a specific commit
    fn find_commit(&'r self, oid: Self::Oid) -> error::Result<Self::Commit, Self::InnerError>;

    /// Retrieve a specific tree
    fn find_tree(&'r self, oid: Self::Oid) -> error::Result<Self::Tree, Self::InnerError>;

    /// Create a new builder for [Self::Commit]s
    fn commit_builder<'c, F>(
        &'r self,
        follow_up: F,
    ) -> error::Result<commit::Builder<'r, 'c, Self, F>, Self::InnerError>
    where
        F: commit::FollowUp<'r, Self>,
        Self: Sized,
        'r: 'c,
    {
        use tree::Builder;

        let author = self.author()?;
        let committer = self.committer()?;
        let tree = self
            .empty_tree_builder()?
            .write()
            .map_err(Into::into)
            .wrap_with_kind(error::Kind::CannotGetTree)?;
        let tree = self.find_tree(tree)?;
        Ok(commit::Builder::new(
            self, author, committer, tree, follow_up,
        ))
    }

    /// Create a new commit
    fn commit<'s>(
        &'r self,
        author: &Self::Signature<'s>,
        committer: &Self::Signature<'s>,
        message: &str,
        tree: &Self::Tree,
        parents: &[&Self::Commit],
    ) -> error::Result<Self::Oid, Self::InnerError>;

    /// Create a tree builder initialized for an empty tree
    fn empty_tree_builder(&'r self) -> error::Result<Self::TreeBuilder, Self::InnerError>;

    /// Create a tree builder initialized for an empty tree
    fn tree_builder(
        &'r self,
        tree: &Self::Tree,
    ) -> error::Result<Self::TreeBuilder, Self::InnerError>;
}

#[cfg(feature = "git2")]
impl<'r> Database<'r> for git2::Repository {
    type Commit = git2::Commit<'r>;
    type Tree = git2::Tree<'r>;
    type Signature<'s> = git2::Signature<'s>;
    type TreeBuilder = git2::TreeBuilder<'r>;

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

    /// Create a new commit
    fn commit<'s>(
        &'r self,
        author: &Self::Signature<'s>,
        committer: &Self::Signature<'s>,
        message: &str,
        tree: &Self::Tree,
        parents: &[&Self::Commit],
    ) -> error::Result<Self::Oid, Self::InnerError> {
        git2::Repository::commit(self, None, author, committer, message, tree, parents)
            .wrap_with_kind(error::Kind::CannotCreateMessage)
    }

    fn empty_tree_builder(&'r self) -> error::Result<Self::TreeBuilder, Self::InnerError> {
        self.treebuilder(None)
            .wrap_with_kind(error::Kind::CannotCreateTreeBuilder)
    }

    fn tree_builder(
        &'r self,
        tree: &Self::Tree,
    ) -> error::Result<Self::TreeBuilder, Self::InnerError> {
        self.treebuilder(Some(tree))
            .wrap_with_kind(error::Kind::CannotCreateTreeBuilder)
    }
}

#[cfg(test)]
pub mod tests;
