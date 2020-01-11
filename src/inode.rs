use std::collections::HashMap;
use std::path::PathBuf;
use std::ffi::OsStr;

pub type Inode = u64;

/// Contains data associated with an inode
struct InodeTableEntry {
    inode: Inode,
    lookups: u64
}

pub struct InodeTable {
    inodes_by_path: HashMap<PathBuf, InodeTableEntry>,
    paths_by_inode: HashMap<Inode, PathBuf>,
    // TODO recycle inodes
    next_inode: Inode
}

impl InodeTable {
    pub fn new() -> InodeTable {
        let mut next_inode = 1;

        let mut inodes_by_path = HashMap::new();
        inodes_by_path.insert(PathBuf::from("/"), InodeTableEntry {
            inode: next_inode,
            lookups: 1,
        });

        let mut paths_by_inode = HashMap::new();
        paths_by_inode.insert(next_inode, PathBuf::from("/"));

        next_inode += 1;

        InodeTable {
            inodes_by_path,
            paths_by_inode,
            next_inode
        }
    }

    /// Returns the inode number and path assigned to the provided parent_ino/name combination.
    pub fn lookup(&mut self, parent_inode: Inode, name: &OsStr) -> (Inode, PathBuf) {
        let parent_path = match self.paths_by_inode.get(&parent_inode) {
            Some(path) => path,
            None => panic!("Attempted lookup on an unknown parent_ino")
        };


        let path: PathBuf = [parent_path, &PathBuf::from(name)].iter().collect();
        match self.inodes_by_path.get_mut(&path) {
            Some(inode) => {
                inode.lookups += 1;
                (inode.inode, path.clone())
            },
            None => {
                let inode = self.next_inode;
                self.inodes_by_path.insert(path.clone(), InodeTableEntry {
                    inode,
                    lookups: 1
                });

                self.next_inode += 1;
                (inode, path.clone())
            }
        }
    }

    /// Forgets the provided inode.
    pub fn forget(&mut self, ino: Inode, nlookups: u64) {
        // inode 1 is special and cannot be forgotten
        if ino == 1 {
            return;
        }

        let path = match self.paths_by_inode.get(&ino) {
            Some(path) => path,
            None => return
        };

        let inode_entry = match self.inodes_by_path.get_mut(path) {
            Some(inode_entry) => inode_entry,
            None => return
        };

        inode_entry.lookups -= nlookups;
        if inode_entry.lookups <= 0 {
            self.inodes_by_path.remove(path);
            self.paths_by_inode.remove(&ino);
        }
    }

    /// Gets the path of the provided inode number.
    pub fn get_path(&mut self, inode: Inode) -> Option<&PathBuf> {
        self.paths_by_inode.get(&inode)
    }

    /// Gets the inode of the provided path.
    /// Path should be relative to the mountpoint.
    pub fn get_inode(&mut self, path: &PathBuf) -> Option<Inode> {
        match self.inodes_by_path.get(path) {
            Some(inode_table_entry) => Some(inode_table_entry.inode),
            None => None
        }
    }
}