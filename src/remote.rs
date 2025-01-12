// git-dit - the distributed issue tracker for git
// Copyright (C) 2017 Matthias Beyer <mail@beyermatthias.de>
// Copyright (C) 2017 Julian Ganz <neither@nut.email>
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.
//

//! Module providing extension trait for remotes
//!

use git2::Remote;

use crate::base::Base;
use issue::Issue;

/// Container for remote names
pub trait Names {
    /// An [Iterator] over remote names
    type NameIter<'n>: Iterator<Item: Name>
    where
        Self: 'n;

    /// Get an [Iterator] over all remotes' names
    fn names(&self) -> Self::NameIter<'_>;
}

impl Names for git2::string_array::StringArray {
    type NameIter<'n> = git2::string_array::Iter<'n>;

    fn names(&self) -> Self::NameIter<'_> {
        self.iter()
    }
}

/// Name of a remote git repository
pub trait Name {
    /// Reference prefix for this repository
    ///
    /// This fn will return the reference prefix of this remote in the form of a
    /// path, like `refs/remotes/<remote-name>`. Its default implementation
    /// returns [None] if [as_str](Self::as_str) would return [None].
    fn ref_path(&self) -> Option<std::path::PathBuf> {
        let mut path = std::path::Path::new(REMOTES_REF_BASE).to_path_buf();
        path.push(self.as_str()?);
        Some(path)
    }

    /// Represenation of this name as a `&str`
    ///
    /// If this name can be represented as a `&str` without loss of information,
    /// this fn will return that representation.
    fn as_str(&self) -> Option<&str>;
}

impl Name for Option<&str> {
    fn as_str(&self) -> Option<&str> {
        *self
    }
}

/// Extension trait for remotes
///
pub trait RemoteExt {
    /// Get the refspec for a specific issue for this remote
    ///
    /// A refspec will only be returned if the remote has a (valid) name.
    fn issue_refspec(&self, issue: Issue<'_, impl Base>) -> Option<String>;

    /// Get the refspec for all issue for this remote
    ///
    /// A refspec will only be returned if the remote has a (valid) name.
    ///
    fn all_issues_refspec(&self) -> Option<String>;
}

impl<'r> RemoteExt for Remote<'r> {
    fn issue_refspec(&self, issue: Issue<'_, impl Base>) -> Option<String> {
        self.name()
            .map(|n| format!("+refs/dit/{0}/*:refs/remotes/{n}/dit/{0}/*", issue.id()))
    }

    fn all_issues_refspec(&self) -> Option<String> {
        self.name()
            .map(|name| format!("+refs/dit/*:refs/remotes/{0}/dit/*", name))
    }
}

const REMOTES_REF_BASE: &str = "refs/remotes/";
