# powerpoint-ooxml

Reading and writing of the PresentationML format (`.pptx`).

Builds on `xml-core` (low-level XML), `opc` (ZIP/OPC container), and `drawing`/`chart` (shape geometry/fill/line/text and embedded charts, reused verbatim for a shape's `<p:spPr>`/`<p:txBody>`). All modeling and generation code for PresentationML elements is original — no dependency on any third-party OOXML crate.

A presentation is a sequence of slides; a slide is a sequence of shapes carrying a text body. Currently modeled: autoshapes, text boxes and placeholders, position/size/rotation and outline geometry, fill and color, line/stroke, visual effects (shadow, glow, reflection, soft edge), rich text inside shapes, pictures, video and audio, embedded charts, grouped shapes, connectors, tables, slide comments, a customizable color/font theme, document properties, and embedded fonts.

Most users should depend on the [`office-toolkit`](https://github.com/Fab2bprog/office-toolkit/tree/main/crates/office-toolkit) facade crate instead of this one directly — see `office_toolkit::powerpoint` and the workspace's [`docs/API_Reference_pptx.md`](https://github.com/Fab2bprog/office-toolkit/blob/main/docs/API_Reference_pptx.md) for the full per-feature reference.

Part of the [office-toolkit](https://github.com/Fab2bprog/office-toolkit) workspace.

## License

Licensed under the [MIT license](https://github.com/Fab2bprog/office-toolkit/blob/main/LICENSE).
