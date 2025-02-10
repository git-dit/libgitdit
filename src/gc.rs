// git-dit - the distributed issue tracker for git
// Copyright (C) 2016, 2017 Matthias Beyer <mail@beyermatthias.de>
// Copyright (C) 2016, 2017 Julian Ganz <neither@nut.email>
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.
//

//! Garbage collecting utilities
//!
//! This module provides git-dit related garbage collection utilites.
//!

use git2::{self, Reference};

use crate::error::{self, ResultExt};
use crate::issue::Issue;
use crate::object;
use crate::reference;
use crate::traversal::{TraversalBuilder, Traversible};
use iter::{self, RefsReferringTo};
use utils::ResultIterExt;

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum ReferenceCollectionSpec {
    Never,
    BackedByRemoteHead,
}

impl Default for ReferenceCollectionSpec {
    fn default() -> Self {
        Self::Never
    }
}

/// Type representing collectable references
///
/// Use this type in order to compute dit-references which are no longer
/// required and thus may be collected.
#[derive(Copy, Clone, Default)]
pub struct CollectableRefs {
    /// Should remote references be considered during collection?
    consider_remote_refs: bool,
    /// Under what circumstances should local heads be collected?
    collect_heads: ReferenceCollectionSpec,
}

impl CollectableRefs {
    /// Causes remote references to be considered
    ///
    /// By default, only local references are considered for deciding which
    /// references will be collected. Calling this function causes the resulting
    /// struct to also consider remote references.
    ///
    pub fn consider_remote_refs(mut self, option: bool) -> Self {
        self.consider_remote_refs = option;
        self
    }

    /// Causes local head references to be collected under a specified condition
    ///
    /// By default, heads are never collected. Using this function a user may
    /// change this behaviour.
    ///
    pub fn collect_heads(mut self, condition: ReferenceCollectionSpec) -> Self {
        self.collect_heads = condition;
        self
    }

    /// Find collectable references for an issue
    ///
    /// Construct an iterator yielding all collectable references for a given
    /// issue, according to the configuration.
    pub fn for_issue<'r, R>(
        &self,
        issue: &Issue<'r, R>,
    ) -> error::Result<impl Iterator<Item = RefResult<'r, R>>, R::InnerError>
    where
        R: reference::Store<'r> + object::Database<'r> + Traversible<'r>,
    {
        let res = self
            .head(issue)?
            .into_iter()
            .map(Ok)
            .chain(self.leaves(issue)?);
        Ok(res)
    }

    /// Retrieve the local reference if it is collectable
    pub fn head<'r, R>(
        &self,
        issue: &Issue<'r, R>,
    ) -> error::Result<Option<R::Reference>, R::InnerError>
    where
        R: reference::Store<'r> + object::Database<'r> + Traversible<'r>,
    {
        use reference::Reference;

        let Some(local_head) = issue.local_head()? else {
            return Ok(None);
        };

        Ok(match self.collect_heads {
            ReferenceCollectionSpec::Never => None,
            ReferenceCollectionSpec::BackedByRemoteHead => {
                let Some(target) = local_head.target() else {
                    return Ok(Some(local_head));
                };

                issue
                    .all_remote_heads()?
                    .try_fold(issue.terminated_messages()?, |i, r| {
                        i.with_heads(r?.target())
                            .map_err(Into::into)
                            .wrap_with_kind(error::Kind::CannotConstructRevwalk)
                    })?
                    .build()
                    .map_err(Into::into)
                    .wrap_with_kind(error::Kind::CannotConstructRevwalk)?
                    .any(|i| i.map(|i| i == target).unwrap_or(false))
                    .then_some(local_head)
            }
        })
    }

    /// Retrieve all collectable leaves for an [Issue]
    pub fn leaves<'r, R>(
        &self,
        issue: &Issue<'r, R>,
    ) -> error::Result<impl Iterator<Item = RefResult<'r, R>>, R::InnerError>
    where
        R: reference::Store<'r> + object::Database<'r> + Traversible<'r>,
    {
        use object::commit::Commit;
        use reference::{Reference, References};

        let mut dead_leaves = Vec::new();

        let mut messages = issue
            .terminated_messages()?
            .with_heads(issue.local_head()?.and_then(|h| h.target()))
            .map_err(Into::into)
            .wrap_with_kind(error::Kind::CannotConstructRevwalk)?;

        let mut candidates: std::collections::HashMap<_, Vec<_>> = Default::default();

        for reference in issue.local_refs()?.leaves() {
            let reference = reference.wrap_with_kind(error::Kind::CannotGetReference)?;
            if let Some(id) = reference.target() {
                messages = messages
                    .with_heads(issue.repo().find_commit(id.clone())?.parent_ids())
                    .map_err(Into::into)
                    .wrap_with_kind(error::Kind::CannotConstructRevwalk)?;

                candidates.entry(id).or_default().push(Ok(reference));
            } else {
                dead_leaves.push(Ok(reference))
            };
        }

        let collectable = messages
            .build()
            .map_err(Into::into)
            .wrap_with_kind(error::Kind::CannotConstructRevwalk)?
            .map_while(move |i| {
                if candidates.is_empty() {
                    // We can stop looking for references to collect when we ran
                    // out of candidates.
                    None
                } else {
                    Some(match i {
                        Ok(id) => candidates.remove(&id).unwrap_or_default(),
                        Err(e) => vec![Err(e)],
                    })
                }
            });
        Ok(std::iter::once(dead_leaves).chain(collectable).flatten())
    }
}

type RefResult<'r, R> = Result<
    <R as reference::Store<'r>>::Reference,
    <<R as Traversible<'r>>::TraversalBuilder as TraversalBuilder>::Error,
>;

#[cfg(test)]
mod tests {
    use super::*;

    use object::commit::Commit;
    use object::tests::TestOdb;
    use object::Database;
    use reference::tests::TestStore;
    use reference::Reference;

    type TestRepo = (TestStore, TestOdb);

    #[test]
    fn collectable_leaves() {
        let repo = TestRepo::default();

        let mut refs_to_collect = Vec::new();
        let mut issues = Vec::new();

        {
            // issue not supposed to be affected
            let initial_message = repo
                .commit_builder(Database::find_commit)
                .expect("Cannot create commit builder")
                .build("Test message 1")
                .expect("Cannot create commit");
            let issue = Issue::new_unchecked(&repo, initial_message.id());
            issue
                .update_head(initial_message.id(), true)
                .expect("Could not update head");
            issue
                .message_builder()
                .expect("Could not create builder")
                .with_parent(initial_message.clone())
                .build("Test message 2")
                .expect("Could not add message");
        }

        {
            let initial_message = repo
                .commit_builder(Database::find_commit)
                .expect("Cannot create commit builder")
                .build("Test message 3")
                .expect("Cannot create commit");
            let issue = Issue::new_unchecked(&repo, initial_message.id());
            issue
                .update_head(initial_message.id(), true)
                .expect("Could not update head");
            let message = issue
                .message_builder()
                .expect("Could not create builder")
                .with_parent(initial_message.clone())
                .build("Test message 4")
                .expect("Could not add message");
            issue
                .update_head(message, true)
                .expect("Could not update head");
            issues.push(issue);
            refs_to_collect.push(message);
        }

        {
            let initial_message = repo
                .commit_builder(Database::find_commit)
                .expect("Cannot create commit builder")
                .build("Test message 5")
                .expect("Cannot create commit");
            let issue = Issue::new_unchecked(&repo, initial_message.id());
            issue
                .update_head(initial_message.id(), true)
                .expect("Could not update head");
            let message1_id = issue
                .message_builder()
                .expect("Could not create builder")
                .with_parent(initial_message.clone())
                .build("Test message 6")
                .expect("Could not add message");
            let message1 = repo
                .find_commit(message1_id)
                .expect("Could not retrieve message");
            issue
                .message_builder()
                .expect("Could not create builder")
                .with_parent(message1)
                .build("Test message 7")
                .expect("Could not add message");
            issues.push(issue);
            refs_to_collect.push(message1_id);
        }

        refs_to_collect.sort();

        let collectable =
            CollectableRefs::default().collect_heads(ReferenceCollectionSpec::BackedByRemoteHead);
        let mut collected: Vec<_> = issues
            .iter()
            .flat_map(|i| {
                collectable
                    .for_issue(i)
                    .expect("Error during discovery of collectable refs")
            })
            .collect::<Result<Vec<_>, _>>()
            .expect("Error during collection")
            .into_iter()
            .map(|r| r.target().expect("Could not retrieve target"))
            .collect();
        collected.sort();
        assert_eq!(refs_to_collect, collected);
    }
}

