// Canvas shortcuts apply while the canvas has focus, not while typing in
// one of its settings.
gpui_kit::actions!(
    flow_editor,
    [
        DeleteSelection,
        SelectAllBlocks,
        CopyBlocks,
        PasteBlocks,
        DuplicateBlocks,
        UndoFlowEdit,
        RedoFlowEdit,
        AddBlock,
        ZoomIn,
        ZoomOut,
        ZoomToFit,
        ArrangeBlocks,
        StopFlow,
    ]
);
