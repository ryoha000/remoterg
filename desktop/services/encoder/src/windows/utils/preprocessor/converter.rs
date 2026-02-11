use anyhow::{Context, Result};
use windows::core::PCSTR;
use windows::Win32::Graphics::Direct3D::Fxc::{D3DCompile, D3DCOMPILE_OPTIMIZATION_LEVEL3};
use windows::Win32::Graphics::Direct3D11::{
    ID3D11ComputeShader, ID3D11DeviceContext, ID3D11Device, ID3D11ShaderResourceView,
    ID3D11UnorderedAccessView,
};

/// Handles RGBA to BGRA conversion using a Compute Shader
pub struct ColorConverter {
    compute_shader: Option<ID3D11ComputeShader>,
}

impl ColorConverter {
    pub fn new() -> Self {
        Self {
            compute_shader: None,
        }
    }

    /// Creates the compute shader if it doesn't exist
    pub fn ensure_shader(&mut self, device: &ID3D11Device) -> Result<()> {
        if self.compute_shader.is_some() {
            return Ok(());
        }

        unsafe {
            // HLSL Compute Shader code (RGBA -> BGRA conversion)
            let shader_code = r#"
                Texture2D<float4> rgba_texture : register(t0);
                RWTexture2D<float4> bgra_texture : register(u0);

                [numthreads(8, 8, 1)]
                void CSMain(uint3 id : SV_DispatchThreadID)
                {
                    float4 rgba = rgba_texture[id.xy];
                    // GPU automatically handles format conversion when writing to BGRA texture
                    bgra_texture[id.xy] = rgba;
                }
            "#;

            let shader_code_bytes = shader_code.as_bytes();
            let entry_point = PCSTR(b"CSMain\0".as_ptr());
            let target = PCSTR(b"cs_5_0\0".as_ptr());

            let mut compiled_shader = None;
            let mut error_blob = None;

            let result = D3DCompile(
                shader_code_bytes.as_ptr() as _,
                shader_code_bytes.len(),
                None,
                None,
                None,
                entry_point,
                target,
                D3DCOMPILE_OPTIMIZATION_LEVEL3 as u32,
                0,
                &mut compiled_shader,
                Some(&mut error_blob),
            );

            if result.is_err() {
                let error_msg = if let Some(blob) = error_blob.as_ref() {
                    let ptr = blob.GetBufferPointer();
                    let len = blob.GetBufferSize();
                    std::str::from_utf8(std::slice::from_raw_parts(ptr as *const u8, len as usize))
                        .unwrap_or("Unknown error")
                } else {
                    "Unknown error"
                };
                return Err(anyhow::anyhow!(
                    "Failed to compile compute shader: {}",
                    error_msg
                ));
            }

            let compiled_shader = compiled_shader.context("Compiled shader is missing")?;
            let buffer_ptr = compiled_shader.GetBufferPointer();
            let buffer_size = compiled_shader.GetBufferSize();
            let shader_bytes = std::slice::from_raw_parts(buffer_ptr as *const u8, buffer_size);

            let mut compute_shader = None;
            device
                .CreateComputeShader(shader_bytes, None, Some(&mut compute_shader))
                .context("Failed to create compute shader")?;

            self.compute_shader = compute_shader;
            Ok(())
        }
    }

    /// Executes the compute shader convert RGBA to BGRA
    pub fn convert(
        &mut self,
        context: &ID3D11DeviceContext,
        srv: &ID3D11ShaderResourceView,
        uav: &ID3D11UnorderedAccessView,
        width: u32,
        height: u32,
    ) -> Result<()> {
        unsafe {
            let shader = self
                .compute_shader
                .as_ref()
                .context("Compute shader not initialized")?;

            // Set Compute Shader
            context.CSSetShader(Some(shader), None);

            // Set SRV and UAV
            let srv_slice = [Some(srv.clone())];
            let uav_slice = [Some(uav.clone())];
            let uav_initial_counts = [0];
            
            context.CSSetShaderResources(0, Some(&srv_slice));
            context.CSSetUnorderedAccessViews(
                0,
                1,
                Some(uav_slice.as_ptr()),
                Some(uav_initial_counts.as_ptr()),
            );

            // Dispatch
            let thread_group_x = (width + 7) / 8;
            let thread_group_y = (height + 7) / 8;
            context.Dispatch(thread_group_x, thread_group_y, 1);

            // Unbind resources
            context.CSSetShader(None, None);
            let null_srv_slice = [None];
            let null_uav_slice = [None];
            let null_uav_initial_counts = [0];
            
            context.CSSetShaderResources(0, Some(&null_srv_slice));
            context.CSSetUnorderedAccessViews(
                0,
                1,
                Some(null_uav_slice.as_ptr()),
                Some(null_uav_initial_counts.as_ptr()),
            );

            Ok(())
        }
    }
}
