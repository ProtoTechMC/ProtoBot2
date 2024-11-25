use serde::Deserialize;
use std::collections::BTreeMap;

pub mod perms_sync;
pub mod smp_commands;

#[derive(Debug, Clone, Deserialize)]
pub struct PterodactylServer {
    pub id: String,
    pub name: String,
    pub category: PterodactylServerCategory,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PterodactylServerCategory {
    Smp,
    Cmp,
    Copy,
    Patreon,
    Protobot,
}

impl PterodactylServerCategory {
    pub fn is_minecraft(&self) -> bool {
        match self {
            Self::Smp | Self::Cmp | Self::Copy | Self::Patreon => true,
            Self::Protobot => false,
        }
    }
}

pub trait PterodactylServerCategoryFilter {
    fn test(&mut self, category: PterodactylServerCategory) -> bool;
}

impl PterodactylServerCategoryFilter for PterodactylServerCategory {
    fn test(&mut self, category: PterodactylServerCategory) -> bool {
        category == *self
    }
}

impl PterodactylServerCategoryFilter for [PterodactylServerCategory] {
    fn test(&mut self, category: PterodactylServerCategory) -> bool {
        self.contains(&category)
    }
}

impl<F> PterodactylServerCategoryFilter for F
where
    F: FnMut(PterodactylServerCategory) -> bool,
{
    fn test(&mut self, category: PterodactylServerCategory) -> bool {
        (*self)(category)
    }
}

#[derive(Debug, Deserialize)]
pub struct PterodactylEmails {
    pub superadmin: Vec<String>,
    pub admin: Vec<String>,
    pub normal: Vec<String>,
    pub ignore: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct PterodactylAllPerms {
    pub superadmin: PterodactylPerms,
    pub admin: PterodactylPerms,
    pub normal: PterodactylPerms,
}

#[derive(Debug, Deserialize)]
pub struct PterodactylPerms {
    default: Vec<String>,
    #[serde(default)]
    overrides: BTreeMap<PterodactylServerCategory, Vec<String>>,
}

impl PterodactylPerms {
    pub fn get_perms(&self, category: PterodactylServerCategory) -> &[String] {
        match self.overrides.get(&category) {
            Some(overrides) => overrides,
            None => &self.default,
        }
    }
}
