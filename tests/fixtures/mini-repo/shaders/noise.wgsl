// vertex shading helpers

/* block comment
   over two lines */

@compute @workgroup_size(1)
fn vertex_normal(idx: u32) -> vec3<f32> {
    // fetch normal
    return vec3<f32>(0.0);
}
