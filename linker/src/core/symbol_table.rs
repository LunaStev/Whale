use object::{ObjectFile, SymbolBinding};
use std::collections::HashMap;

/// Local names belong to one input object; exported names share a link namespace.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum SymbolKey {
    Global(String),
    Local { object_index: usize, name: String },
}

#[derive(Default)]
pub struct SymbolTable {
    pub symbols: HashMap<SymbolKey, ResolvedSymbol>,
}

pub struct ResolvedSymbol {
    pub name: String,
    pub object_index: Option<usize>,
    pub section_index: Option<usize>,
    pub value: u64,
    pub size: u64,
    pub binding: SymbolBinding,
}

impl SymbolTable {
    pub fn new() -> Self {
        Self {
            symbols: HashMap::new(),
        }
    }

    pub fn resolve(&mut self, objects: &[ObjectFile]) -> Result<(), String> {
        // A resolution attempt describes only these inputs, including on failure.
        self.symbols.clear();
        let mut symbols: HashMap<SymbolKey, ResolvedSymbol> = HashMap::new();
        for (obj_idx, obj) in objects.iter().enumerate() {
            for sym in &obj.symbols {
                if sym.section_index.is_some() {
                    let key = if sym.binding == SymbolBinding::Local {
                        SymbolKey::Local {
                            object_index: obj_idx,
                            name: sym.name.clone(),
                        }
                    } else {
                        SymbolKey::Global(sym.name.clone())
                    };
                    if let Some(existing) = symbols.get(&key) {
                        if sym.binding == SymbolBinding::Local {
                            return Err(format!(
                                "Duplicate local symbol in object {obj_idx}: {}",
                                sym.name
                            ));
                        }
                        if existing.section_index.is_some() && sym.binding == SymbolBinding::Global
                        {
                            return Err(format!("Duplicate global symbol: {}", sym.name));
                        }
                    }
                    symbols.insert(
                        key,
                        ResolvedSymbol {
                            name: sym.name.clone(),
                            object_index: Some(obj_idx),
                            section_index: sym.section_index,
                            value: sym.value,
                            size: sym.size,
                            binding: sym.binding,
                        },
                    );
                }
            }
        }
        self.symbols = symbols;
        Ok(())
    }
}
