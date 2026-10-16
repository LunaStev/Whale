use super::archive::ArchiveObject;

use object::{ObjectFile, SymbolBinding};

use std::collections::HashSet;

pub enum LinkInput {
    Object(ObjectFile),
    Archive(Vec<ArchiveObject>),
}

/// Unix left-to-right archive semantics; rescan each encountered archive to a
/// fixed point. Later files do not cause earlier archives to be revisited.
pub fn select(
    inputs: &[LinkInput],
    entry: &str,
    max_bytes: u64,
) -> Result<Vec<ObjectFile>, String> {
    let mut selected = Vec::new();
    let mut fuel = 10_000_000usize;
    let mut payload_bytes = 0u64;

    for input in inputs {
        match input {
            LinkInput::Object(object) => {
                if selected.len() >= 4096 {
                    return Err("selected object limit exceeded".into());
                }
                check_payload(object, &mut payload_bytes, max_bytes)?;
                selected.push(object.clone());
            },
            LinkInput::Archive(members) => {
                let mut used = vec![false; members.len()];
                loop {
                    let demand = unresolved(&selected, entry, &mut fuel)?;
                    let mut found = None;
                    for (i, member) in members.iter().enumerate() {
                        if used[i] {
                            continue;
                        }
                        for symbol in &member.object.symbols {
                            charge(&mut fuel)?;
                            if symbol.section_index.is_some()
                                && symbol.binding != SymbolBinding::Local
                                && demand.contains(&symbol.name)
                            {
                                found = Some(i);
                                break;
                            }
                        }
                        if found.is_some() {
                            break;
                        }
                    }

                    let Some(index) = found else {
                        break;
                    };

                    used[index] = true;
                    check_payload(&members[index].object, &mut payload_bytes, max_bytes)?;
                    selected.push(members[index].object.clone());

                    if selected.len() > 4096 {
                        return Err("selected object limit exceeded".into());
                    }
                }
            },
        }
    }

    Ok(selected)
}

fn charge(fuel: &mut usize) -> Result<(), String> {
    *fuel = fuel.checked_sub(1).ok_or("archive selection work budget exceeded")?;
    Ok(())
}

fn unresolved(
    objects: &[ObjectFile],
    entry: &str,
    fuel: &mut usize,
) -> Result<HashSet<String>, String> {
    let mut definitions = HashSet::new();
    for object in objects {
        for symbol in &object.symbols {
            charge(fuel)?;
            if symbol.section_index.is_some() && symbol.binding != SymbolBinding::Local {
                definitions.insert(symbol.name.as_str());
            }
        }
    }

    let mut needed = HashSet::new();
    if !definitions.contains(entry) {
        needed.insert(entry.to_owned());
    }

    for object in objects {
        let locals: HashSet<_> = object
            .symbols
            .iter()
            .filter(|s| s.binding == SymbolBinding::Local && s.section_index.is_some())
            .map(|s| s.name.as_str())
            .collect();

        let weak: HashSet<_> = object
            .symbols
            .iter()
            .filter(|s| s.binding == SymbolBinding::Weak && s.section_index.is_none())
            .map(|s| s.name.as_str())
            .collect();

        for relocation in &object.relocations {
            charge(fuel)?;
            if !definitions.contains(relocation.symbol.as_str())
                && !locals.contains(relocation.symbol.as_str())
                && !weak.contains(relocation.symbol.as_str())
            {
                needed.insert(relocation.symbol.clone());
            }
        }
    }

    Ok(needed)
}

fn check_payload(object: &ObjectFile, total: &mut u64, limit: u64) -> Result<(), String> {
    for section in &object.sections {
        *total = total
            .checked_add(section.data.len() as u64)
            .filter(|n| *n <= limit)
            .ok_or("selected payload byte limit exceeded")?;
    }

    Ok(())
}
