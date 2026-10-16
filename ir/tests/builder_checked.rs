use ir::*;

#[test]
fn checked_entry_allocation_cannot_mutate_a_terminated_entry() {
    let mut mb = ModuleBuilder::new("x86_64-whale-linux", DataLayout::default_64bit_le());
    let mut f = mb.begin_function("entry_allocation", vec![], Type::Void);
    let entry = f.entry_block();
    let next = f.create_block("next");
    f.try_br(next).unwrap();
    f.try_set_insert_point(next).unwrap();
    assert!(
        matches!(f.try_alloca_in_entry(Type::I32,4),Err(BuilderError::TerminatedBlock(id)) if id==entry)
    );
    f.try_ret(None).unwrap();
    f.finish();
    let m = mb.finish();
    assert!(m.functions[0].blocks.iter().find(|b| b.id == entry).unwrap().instructions.is_empty());
    verify_module(&m).unwrap();
}

#[test]
fn checked_ids_and_failed_insertions_preserve_the_program() {
    let target = Target::X86_64WhaleLinux;
    let mut module = ModuleBuilder::new(target.name(), target.data_layout());
    let mut f = module.begin_function("checked", vec![("arg".into(), Type::I32)], Type::I32);
    let entry = f.entry_block();
    assert_eq!(f.try_param_value(1), Err(BuilderError::UnknownParameter { index: 1, count: 1 }));
    assert_eq!(f.try_set_insert_point(BlockId(999)), Err(BuilderError::UnknownBlock(BlockId(999))));
    let arg = f.try_param_value(0).unwrap();
    assert_eq!(
        f.try_add(Type::I32, arg, ValueId(999)),
        Err(BuilderError::UnknownValue(ValueId(999)))
    );
    assert_eq!(f.try_br(BlockId(999)), Err(BuilderError::UnknownBlock(BlockId(999))));
    let result = f.try_add(Type::I32, arg, arg).unwrap();
    f.try_ret(Some(result)).unwrap();
    assert_eq!(f.try_ret(Some(arg)), Err(BuilderError::TerminatedBlock(entry)));
    assert_eq!(f.try_const_i32(2), Err(BuilderError::TerminatedBlock(entry)));
    assert_eq!(f.try_store(Type::I32, arg, arg, 4), Err(BuilderError::TerminatedBlock(entry)));
    assert_eq!(f.try_trap("replace"), Err(BuilderError::TerminatedBlock(entry)));
    f.finish();
    let module = module.finish();
    verify_module(&module).unwrap();
    let f = &module.functions[0];
    assert_eq!(f.value_types.len(), 2, "failed operations cannot consume value IDs");
    assert_eq!(f.blocks[0].instructions.len(), 1);
    assert!(
        matches!(f.blocks[0].terminator, Some(Terminator::Ret { value: Some(v), .. }) if v == result)
    );
}

#[test]
fn checked_calls_report_terminated_blocks_and_can_build_neighbors() {
    let target = Target::X86_64WhaleLinux;
    let mut m = ModuleBuilder::new(target.name(), target.data_layout());
    let signature = FunctionSignature::whale(vec![], Type::Void);
    let id = m
        .declare_function("external", signature.clone(), Linkage::External, Some("external".into()))
        .unwrap();
    let mut f = m.begin_function("caller", vec![], Type::Void);
    f.call(Callee::Direct(id), vec![]).unwrap();
    let entry = f.entry_block();
    f.try_ret(None).unwrap();
    assert_eq!(
        f.call(Callee::Direct(id), vec![]),
        Err(CallError::Builder(BuilderError::TerminatedBlock(entry)))
    );
    assert_eq!(
        f.null_function(signature),
        Err(CallError::Builder(BuilderError::TerminatedBlock(entry)))
    );
    let next = f.create_block("unreachable");
    f.try_set_insert_point(next).unwrap();
    f.try_trap("unreachable").unwrap();
    f.finish();
    verify_module(&m.finish()).unwrap();
}
