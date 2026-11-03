use audit_extract::{analyzer_for, CallKind, FunctionKind};

const RUST_SRC: &str = r#"//! Module doc.

/// Doc for integrate_rk4.
/// Second line.
pub fn integrate_rk4(velocity: f32, dt: f32) -> f32 {
    // inner
    let v = velocity * dt;
    println!("v={v}");
    Vec::new()
}

struct Body { mass: f32 }

impl Body {
    /// Apply force.
    fn apply_force(&mut self, force: f32) {
        self.mass += force;
        self.clamp();
        integrate_rk4(self.mass, 0.1);
    }

    fn clamp(&self) {}
}
"#;

const PY_SRC: &str = r#"# module comment

def triangulate(points):
    """Delaunay triangulation."""
    return points

class Solver:
    def solve_linear(self, matrix: list) -> list:
        """Solve a linear system."""
        self.result = matrix
        return self.result
"#;

const WGSL_SRC: &str = r#"// vertex shading helpers

@compute @workgroup_size(1)
fn vertex_normal(idx: u32) -> vec3<f32> {
    let n = normalize(vec3<f32>(0.0));
    return n;
}
"#;

#[test]
fn rust_functions_and_docs() {
    let a = analyzer_for("rust").unwrap().analyze(RUST_SRC).unwrap();
    let names: Vec<&str> = a.functions.iter().map(|f| f.name.as_str()).collect();
    assert!(names.contains(&"integrate_rk4"));
    assert!(names.contains(&"apply_force"));

    let f = a
        .functions
        .iter()
        .find(|f| f.name == "integrate_rk4")
        .unwrap();
    assert_eq!(f.kind, FunctionKind::Function);
    assert_eq!(f.doc.as_deref(), Some("Doc for integrate_rk4.\nSecond line."));
    assert!(f.signature.contains("fn integrate_rk4"));

    let m = a
        .functions
        .iter()
        .find(|f| f.name == "apply_force")
        .unwrap();
    assert_eq!(m.kind, FunctionKind::Method);
    assert_eq!(m.scope.as_deref(), Some("impl Body"));

    assert!(a.comments.iter().any(|c| c.text.contains("Module doc")));
    assert!(a.comments.iter().any(|c| c.text == "inner"));
    assert!(a.comments.iter().filter(|c| c.is_doc).count() >= 3);
}

#[test]
fn python_functions_docstrings_and_methods() {
    let a = analyzer_for("python").unwrap().analyze(PY_SRC).unwrap();
    let f = a
        .functions
        .iter()
        .find(|f| f.name == "triangulate")
        .unwrap();
    assert_eq!(f.doc.as_deref(), Some("Delaunay triangulation."));
    assert_eq!(f.kind, FunctionKind::Function);

    let m = a
        .functions
        .iter()
        .find(|f| f.name == "solve_linear")
        .unwrap();
    assert_eq!(m.kind, FunctionKind::Method);
    assert_eq!(m.scope.as_deref(), Some("class Solver"));

    assert!(a.comments.iter().any(|c| c.text == "module comment"));
}

#[test]
fn wgsl_function_and_comments() {
    let a = analyzer_for("wgsl").unwrap().analyze(WGSL_SRC).unwrap();
    let f = a.functions.iter().find(|f| f.name == "vertex_normal").unwrap();
    assert!(f.signature.contains("@compute"));
    assert!(f.signature.contains("fn vertex_normal"));
    assert!(a
        .comments
        .iter()
        .any(|c| c.text == "vertex shading helpers"));
}

#[test]
fn rust_calls_types_and_fields() {
    let a = analyzer_for("rust").unwrap().analyze(RUST_SRC).unwrap();

    let f = a
        .functions
        .iter()
        .find(|f| f.name == "integrate_rk4")
        .unwrap();
    assert_eq!(f.body.param_types, vec!["velocity: f32", "dt: f32"]);
    assert_eq!(f.body.return_type.as_deref(), Some("f32"));
    let names: Vec<&str> = f.body.calls.iter().map(|c| c.name.as_str()).collect();
    assert!(names.contains(&"println"), "macro call captured: {names:?}");
    assert!(names.contains(&"new"), "scoped call captured: {names:?}");
    let mac = f.body.calls.iter().find(|c| c.name == "println").unwrap();
    assert_eq!(mac.kind, CallKind::Macro);
    let sc = f.body.calls.iter().find(|c| c.name == "new").unwrap();
    assert_eq!(sc.kind, CallKind::Scoped);
    assert_eq!(sc.qualifier.as_deref(), Some("Vec"));

    let m = a
        .functions
        .iter()
        .find(|f| f.name == "apply_force")
        .unwrap();
    assert!(m.body.mutates_receiver, "&mut self detected");
    assert!(m.body.writes_fields.contains(&"mass".to_string()));
    assert!(m.body.reads_fields.contains(&"mass".to_string()));
    let cn: Vec<&str> = m.body.calls.iter().map(|c| c.name.as_str()).collect();
    assert!(cn.contains(&"clamp"));
    assert!(cn.contains(&"integrate_rk4"));
    let mc = m.body.calls.iter().find(|c| c.name == "clamp").unwrap();
    assert_eq!(mc.kind, CallKind::Method);
    assert_eq!(mc.qualifier.as_deref(), Some("self"));
}

#[test]
fn python_calls_types_and_self_fields() {
    let a = analyzer_for("python").unwrap().analyze(PY_SRC).unwrap();
    let m = a
        .functions
        .iter()
        .find(|f| f.name == "solve_linear")
        .unwrap();
    assert_eq!(m.body.return_type.as_deref(), Some("list"));
    assert!(m.body.param_types.iter().any(|p| p == "matrix: list"));
    assert!(m.body.writes_fields.contains(&"result".to_string()));
    assert!(m.body.mutates_receiver);
}

#[test]
fn wgsl_calls_and_types() {
    let a = analyzer_for("wgsl").unwrap().analyze(WGSL_SRC).unwrap();
    let f = a
        .functions
        .iter()
        .find(|f| f.name == "vertex_normal")
        .unwrap();
    assert_eq!(f.body.return_type.as_deref(), Some("vec3<f32>"));
    assert_eq!(f.body.param_types, vec!["idx: u32"]);
    let names: Vec<&str> = f.body.calls.iter().map(|c| c.name.as_str()).collect();
    assert!(names.contains(&"normalize"), "fn call: {names:?}");
    assert!(names.contains(&"vec3"), "type ctor call: {names:?}");
}
