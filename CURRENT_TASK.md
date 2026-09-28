# CURRENT TASK: Smart Magnetic Connectors (Milestone 9)

## Goal
Implement extraction and rendering for OpenXML Connector shapes (`Shape::Connector`), preserving their start/end connection sites (`stCxn`, `endCxn`) and rendering them dynamically on the slide canvas.

## Exact Types & Definitions (DO NOT search .cargo/registry!)

In `office_toolkit::powerpoint` (already imported in `inspect.rs`):
```rust
pub struct Connector {
    pub id: u32,
    pub name: String,
    pub properties: ShapeProperties,
    pub start_connection: Option<ShapeConnection>,
    pub end_connection: Option<ShapeConnection>,
}

pub struct ShapeConnection {
    pub shape_id: u32,
    pub index: u32,
}
```

## Step 1: Update `crates/omashow-core/src/inspect.rs`

1. Add `ConnectionInfo` and `ConnectorInfo` structs near `TableInfo`:
```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConnectionInfo {
    pub shape_id: u32,
    pub index: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConnectorInfo {
    pub start_connection: Option<ConnectionInfo>,
    pub end_connection: Option<ConnectionInfo>,
}
```

2. Add `connector` field to `ShapeInfo`:
```rust
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connector: Option<ConnectorInfo>,
```
Add `connector: None` to all existing `ShapeInfo` constructors (AutoShape, Picture, Chart, Group, Table, Media).

3. In the `Shape::Connector(c)` arm of `shape_info()`:
```rust
    Shape::Connector(c) => ShapeInfo {
        id: c.id,
        name: c.name.clone(),
        kind: "connector",
        placeholder: None,
        bounds: c
            .properties
            .transform
            .as_ref()
            .and_then(|t| match (t.offset, t.extent) {
                (Some((x, y)), Some((w, h))) => Some(ctx.map_box(x, y, w, h)),
                _ => None,
            }),
        text: None,
        fill: c.properties.fill.as_ref().map(fill_to_css),
        line: c.properties.line.as_ref().map(line_info),
        runs: Vec::new(),
        pic: None,
        table: None,
        connector: Some(ConnectorInfo {
            start_connection: c.start_connection.map(|sc| ConnectionInfo {
                shape_id: sc.shape_id,
                index: sc.index,
            }),
            end_connection: c.end_connection.map(|ec| ConnectionInfo {
                shape_id: ec.shape_id,
                index: ec.index,
            }),
        }),
        children: None,
    },
```

## Step 2: Update `apps/omashow-tauri/src/frontend/slide_render.js`

In `drawShapes(container, shapes, pxPerEmu, pxPerInch, mini)`, add handling for `sh.kind === "connector"`:
```javascript
    if (sh.kind === "connector") {
      const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
      svg.className = "slide-connector";
      svg.dataset.shapeId = sh.id;
      svg.style.position = "absolute";
      svg.style.left = "0";
      svg.style.top = "0";
      svg.style.width = "100%";
      svg.style.height = "100%";
      svg.style.pointerEvents = "none";
      svg.style.overflow = "visible";

      // If connected to shapes, resolve anchor points; otherwise use bounds
      let x1 = 0, y1 = 0, x2 = 0, y2 = 0;
      if (sh.bounds) {
        x1 = sh.bounds.x * pxPerEmu;
        y1 = sh.bounds.y * pxPerEmu;
        x2 = (sh.bounds.x + sh.bounds.w) * pxPerEmu;
        y2 = (sh.bounds.y + sh.bounds.h) * pxPerEmu;
      }
      if (sh.connector) {
        if (sh.connector.start_connection) {
          const src = shapes.find(s => s.id === sh.connector.start_connection.shape_id);
          if (src && src.bounds) {
            x1 = (src.bounds.x + src.bounds.w / 2) * pxPerEmu;
            y1 = (src.bounds.y + src.bounds.h / 2) * pxPerEmu;
          }
        }
        if (sh.connector.end_connection) {
          const dst = shapes.find(s => s.id === sh.connector.end_connection.shape_id);
          if (dst && dst.bounds) {
            x2 = (dst.bounds.x + dst.bounds.w / 2) * pxPerEmu;
            y2 = (dst.bounds.y + dst.bounds.h / 2) * pxPerEmu;
          }
        }
      }

      const line = document.createElementNS("http://www.w3.org/2000/svg", "line");
      line.setAttribute("x1", x1);
      line.setAttribute("y1", y1);
      line.setAttribute("x2", x2);
      line.setAttribute("y2", y2);
      const strokeColor = themeColor(sh.line && sh.line.color) || "#94a3b8";
      const strokeWidth = sh.line && sh.line.width_emu ? Math.max(1, (sh.line.width_emu / 914400) * pxPerInch) : 2;
      line.setAttribute("stroke", strokeColor);
      line.setAttribute("stroke-width", strokeWidth);
      svg.appendChild(line);
      container.appendChild(svg);
      continue;
    }
```

## Step 3: Add Unit Test in `crates/omashow-core/tests/inspect.rs`

```rust
#[test]
fn connector_extraction_with_connections() {
    use office_toolkit::powerpoint::{Connector, ShapeConnection};
    use office_toolkit::drawing::{ShapeProperties, Transform2D};

    let mut props = ShapeProperties::new();
    let mut transform = Transform2D::new();
    transform.offset = Some((100_000, 100_000));
    transform.extent = Some((500_000, 500_000));
    props.transform = Some(transform);

    let conn = Connector::new(101, "Arrow Connector")
        .with_properties(props)
        .with_start_connection(1, 0)
        .with_end_connection(2, 2);

    let mut pres = Presentation::new();
    let slide = Slide::new().with_shape(Shape::Connector(conn));
    pres.slides.push(slide);

    let shapes = get_slide_shapes(&pres, 0).expect("shapes");
    assert_eq!(shapes.len(), 1);
    let s = &shapes[0];
    assert_eq!(s.kind, "connector");
    let cinfo = s.connector.as_ref().expect("connector info");
    assert_eq!(cinfo.start_connection, Some(omashow_core::inspect::ConnectionInfo { shape_id: 1, index: 0 }));
    assert_eq!(cinfo.end_connection, Some(omashow_core::inspect::ConnectionInfo { shape_id: 2, index: 2 }));
}
```

## Step 4: Verification & Commit
- Run `cargo test -p omashow-core`
- Run `cargo check --workspace`
- Mark `[x] Smart Magnetic Connectors` in TODO.md
- Make git commit: `feat(connector): add smart magnetic connector extraction and SVG canvas rendering`
