// git-dit - the distributed issue tracker for git
// Copyright (C) 2016, 2017 Matthias Beyer <mail@beyermatthias.de>
// Copyright (C) 2016, 2017 Julian Ganz <neither@nut.email>
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.
//

//! Utility iterators
//!
//! This module provides various iterators.
//!

use git2::{self, Repository};
use std::collections::HashMap;

use issue;
use repository::RepositoryExt;

use error::*;
use error::Kind as EK;

/// Iterator for transforming the names of head references to issues
///
/// This iterator wrapps a `ReferenceNames` iterator and returns issues
/// associated to the head references returned by the wrapped iterator.
///
pub struct HeadRefsToIssuesIter<'r>
{
    inner: git2::References<'r>,
    repo: &'r Repository
}

impl<'r> HeadRefsToIssuesIter<'r>
{
    pub fn new(repo: &'r Repository, inner: git2::References<'r>) -> Self {
        HeadRefsToIssuesIter { inner: inner, repo: repo }
    }
}

impl<'r> Iterator for HeadRefsToIssuesIter<'r> {
    type Item = Result<issue::Issue<'r, git2::Repository>, git2::Error>;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner
            .next()
            .map(|reference| {
                reference
                    .wrap_with_kind(EK::CannotGetReference)
                    .and_then(|r| self.repo.issue_by_head_ref(&r))
            })
    }
}

/// Iterator over references referring to any of a number of commits
///
/// This iterator wraps a `git2::Revwalk`. It will iterate over the commits
/// provided by the wrapped iterator. If one of those commits is referred to
/// by any of the whatched references, that references will be returned.
///
/// Only "watched" references are returned, e.g. they need to be supplied
/// through the `watch_ref()` function. Each reference will only be returned
/// once.
///
pub struct RefsReferringTo<'r> {
    refs: HashMap<git2::Oid, Vec<git2::Reference<'r>>>,
    inner: git2::Revwalk<'r>,
    current_refs: Vec<git2::Reference<'r>>,
}

impl<'r> RefsReferringTo<'r> {
    /// Create a new iterator iterating over the messages supplied
    ///
    pub fn new(messages: git2::Revwalk<'r>) -> Self
    {
        Self { refs: HashMap::new(), inner: messages, current_refs: Vec::new() }
    }

    /// Push a starting point for the iteration
    ///
    /// The message will be pushed onto the underlying `Revwalk` used for
    /// iterating over messages.
    ///
    pub fn push(&mut self, message: git2::Oid) -> Result<(), git2::Error> {
        self.inner.push(message).wrap_with_kind(EK::CannotConstructRevwalk)
    }

    /// Start watching a reference
    ///
    /// A watched reference may be returned by the iterator.
    ///
    pub fn watch_ref(&mut self, reference: git2::Reference<'r>) -> Result<(), git2::Error> {
        let id = reference
            .peel(git2::ObjectType::Any)
            .wrap_with(|| EK::CannotGetCommitForRev(reference.name().unwrap_or_default().to_string()))?
            .id();
        self.refs.entry(id).or_insert_with(Vec::new).push(reference);
        Ok(())
    }

    /// Start watching a number of references
    ///
    pub fn watch_refs<I>(&mut self, references: I) -> Result<(), git2::Error>
        where I: IntoIterator<Item = git2::Reference<'r>>
    {
        for reference in references.into_iter() {
            self.watch_ref(reference)?;
        }
        Ok(())
    }
}

impl<'r> Iterator for RefsReferringTo<'r> {
    type Item = Result<git2::Reference<'r>, git2::Error>;

    fn next(&mut self) -> Option<Self::Item> {
        'outer: loop {
            if let Some(reference) = self.current_refs.pop() {
                // get one of the references for the current commit
                return Some(Ok(reference));
            }

            // Refills may be rather expensive. Let's check whether we have any
            // refs left, first.
            if self.refs.is_empty() {
                return None;
            }

            // refill the stash of references for the next commit
            for item in &mut self.inner {
                match item.wrap_with_kind(EK::CannotGetCommit) {
                    Ok(id) => if let Some(new_refs) = self.refs.remove(&id) {
                        // NOTE: should new_refs be empty, we just loop once
                        //       more through the 'outer loop
                        self.current_refs = new_refs;
                        continue 'outer;
                    },
                    Err(err) => return Some(Err(err)),
                }
            }

            // We depleted the inner iterator.
            return None;
        }
    }
}


/// Implementation of Extend for RefsReferringTo
///
/// The references supplied will be returned by the extended `RefsReferringTo`
/// iterator.
///
impl<'r> Extend<git2::Reference<'r>> for RefsReferringTo<'r> {
    fn extend<I>(&mut self, references: I)
        where I: IntoIterator<Item = git2::Reference<'r>>
    {
        self.current_refs.extend(references);
    }
}


/// Iterator for deleting references
///
/// This iterator wraps an iterator over references. All of the references
/// returned by the wrapped iterator are deleted. The `ReferenceDeletingIter`
/// itself returns (only) the errors encountered. Sucessful deletions are not
/// reported, e.g. no items will be returned.
///
/// Use this iterator if you want to remove references from a repository but
/// also want to delegate the decision what to do if an error is encountered.
///
pub struct ReferenceDeletingIter<'r, I>
    where I: Iterator<Item = git2::Reference<'r>>
{
    inner: I
}

impl<'r, I> ReferenceDeletingIter<'r, I>
    where I: Iterator<Item = git2::Reference<'r>>
{
    /// Delete, ignoring errors
    ///
    /// Delete all references returned by the wrapped iterator, ignoring all
    /// errors.
    ///
    pub fn delete_ignoring(self) {
        for _ in self {}
    }
}

impl<'r, I, J> From<J> for ReferenceDeletingIter<'r, I>
    where I: Iterator<Item = git2::Reference<'r>>,
          J: IntoIterator<Item = git2::Reference<'r>, IntoIter = I>
{
    fn from(items: J) -> Self {
        ReferenceDeletingIter { inner: items.into_iter() }
    }
}

impl<'r, I> Iterator for ReferenceDeletingIter<'r, I>
    where I: Iterator<Item = git2::Reference<'r>>
{
    type Item = Error<git2::Error>;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner
            .by_ref()
            .filter_map(|mut r| r
                .delete()
                .wrap_with(|| EK::CannotDeleteReference(r.name().unwrap_or_default().to_string()))
                .err()
            )
            .next()
    }
}




#[cfg(test)]
mod tests {
    use super::*;
    use test_utils::{TestingRepo, empty_tree};

    // RefsReferringTo tests

    #[test]
    fn referred_refs() {
        let mut testing_repo = TestingRepo::new("referred_refs");
        let repo = testing_repo.repo();

        let sig = git2::Signature::now("Foo Bar", "foo.bar@example.com")
            .expect("Could not create signature");
        let empty_tree = empty_tree(repo);
        let empty_parents: Vec<&git2::Commit> = vec![];

        let mut commits = repo.revwalk().expect("Could not create revwalk");
        let mut refs_to_watch = Vec::new();
        let mut refs_to_report = Vec::new();

        {
            let commit = repo
                .commit(None, &sig, &sig, "Test message 1", &empty_tree, &empty_parents)
                .expect("Could not create commit");
            let refa = repo
                .reference("refs/test/1a", commit, false, "create test ref 1a")
                .expect("Could not create reference");
            let refb = repo
                .reference("refs/test/1b", commit, false, "create test ref 1b")
                .expect("Could not create reference");
            commits.push(commit).expect("Could not push commit onto revwalk");
            refs_to_report.push(refa.name().expect("Could not retrieve name").to_string());
            refs_to_report.push(refb.name().expect("Could not retrieve name").to_string());
            refs_to_watch.push(refa);
            refs_to_watch.push(refb);
        }

        {
            let commit = repo
                .commit(None, &sig, &sig, "Test message 2", &empty_tree, &empty_parents)
                .expect("Could not create commit");
            let refa = repo
                .reference("refs/test/2a", commit, false, "create test ref 2a")
                .expect("Could not create reference");
            repo.reference("refs/test/2b", commit, false, "create test ref 2b")
                .expect("Could not create reference");
            commits.push(commit).expect("Could not push commit onto revwalk");
            refs_to_report.push(refa.name().expect("Could not retrieve name").to_string());
            refs_to_watch.push(refa);
        }

        {
            let commit = repo
                .commit(None, &sig, &sig, "Test message 3", &empty_tree, &empty_parents)
                .expect("Could not create commit");
            repo.reference("refs/test/3a", commit, false, "create test ref 3a")
                .expect("Could not create reference");
            repo.reference("refs/test/3b", commit, false, "create test ref 3b")
                .expect("Could not create reference");
            commits.push(commit).expect("Could not push commit onto revwalk");
        }

        {
            let commit = repo
                .commit(None, &sig, &sig, "Test message 4", &empty_tree, &empty_parents)
                .expect("Could not create commit");
            let refa = repo
                .reference("refs/test/4a", commit, false, "create test ref 4a")
                .expect("Could not create reference");
            let refb = repo
                .reference("refs/test/4b", commit, false, "create test ref 4b")
                .expect("Could not create reference");
            refs_to_watch.push(refa);
            refs_to_watch.push(refb);
        }

        let mut referred = RefsReferringTo::new(commits);
        referred.watch_refs(refs_to_watch).expect("Could not watch refs");

        let mut reported: Vec<_> = referred
            .map(|item| item
                .expect("Error during iterating over refs")
                .name()
                .expect("Could not retrieve name")
                .to_string()
            )
            .collect();
        reported.sort();
        refs_to_report.sort();
        assert_eq!(reported, refs_to_report);
    }
}

