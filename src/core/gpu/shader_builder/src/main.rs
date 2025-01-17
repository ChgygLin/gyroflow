use naga::back::glsl;
use naga::front::spv;
use naga::valid::*;

use std::error::Error;
trait PrettyResult {
    type Target;
    fn unwrap_pretty(self) -> Self::Target;
}
impl<T, E: Error> PrettyResult for Result<T, E> {
    type Target = T;
    fn unwrap_pretty(self) -> T {
        match self {
            Result::Ok(value) => value,
            Result::Err(error) => {
                eprint!("{error}");
                let mut e = error.source();
                if e.is_some() { eprintln!(": "); } else { eprintln!(); }
                while let Some(source) = e {
                    eprintln!("\t{source}");
                    e = source.source();
                }
                std::process::exit(1);
            }
        }
    }
}

fn main() {
    let main_shader_path     = env!("stabilize_f32");
    let main_u32_shader_path = env!("stabilize_u32");
    let glsl_shader_path     = env!("stabilize_qtrhi");

    let main_shader = std::fs::read(&main_shader_path).unwrap();
    let main_u32_shader = std::fs::read(&main_u32_shader_path).unwrap();
    let glsl_shader = std::fs::read(&glsl_shader_path).unwrap();
    println!("SPIR-V shader len: {}, {main_shader_path}", main_shader.len());
    println!("SPIR-V shader (u32) len: {}, {main_u32_shader_path}", main_u32_shader.len());
    println!("GLSL shader len: {}, {glsl_shader_path}", glsl_shader.len());

    let in_spv_options = spv::Options {
        adjust_coordinate_space: false,
        strict_capabilities: true,
        block_ctx_dump_prefix: None,
    };

    let spirv_out_path     = format!("{}/../compiled/stabilize.spv", env!("CARGO_MANIFEST_DIR"));
    let spirv_u32_out_path = format!("{}/../compiled/stabilize_u32.spv", env!("CARGO_MANIFEST_DIR"));
    let frag_out_path      = format!("{}/../compiled/stabilize.spv.frag", env!("CARGO_MANIFEST_DIR"));
    let qsb_out_path       = format!("{}/../compiled/stabilize.frag.qsb", env!("CARGO_MANIFEST_DIR"));
    // let wgsl_out_path  = format!("{}/../compiled/stabilize.spv.wgsl", env!("CARGO_MANIFEST_DIR"));

    println!("Resulting SPIR-V: {spirv_out_path:?}");
    println!("Resulting SPIR-V (u32): {spirv_u32_out_path:?}");
    println!("Resulting FRAG: {frag_out_path:?}");
    println!("Resulting QSB: {qsb_out_path:?}");
    // println!("Resulting WGSL: {wgsl_out_path:?}");

    std::fs::write(&spirv_out_path, main_shader).unwrap();
    std::fs::write(&spirv_u32_out_path, main_u32_shader).unwrap();

    // Emit HLSL
    /*{
        let module = spv::parse_u8_slice(&glsl_shader, &in_spv_options).unwrap();
        let info = Validator::new(ValidationFlags::default(), Capabilities::all()).validate(&module).unwrap_pretty();

        let options = naga::back::hlsl::Options {
            shader_model: naga::back::hlsl::ShaderModel::V5_1,
            binding_map: naga::back::hlsl::BindingMap::from([
                (naga::ResourceBinding { group: 0, binding: 1 }, naga::back::hlsl::BindTarget { space: 0, register: 1, ..Default::default() }),
                (naga::ResourceBinding { group: 0, binding: 2 }, naga::back::hlsl::BindTarget { space: 0, register: 0, ..Default::default() }), // KernelParams
                (naga::ResourceBinding { group: 0, binding: 3 }, naga::back::hlsl::BindTarget { space: 0, register: 0, ..Default::default() }),
                (naga::ResourceBinding { group: 0, binding: 4 }, naga::back::hlsl::BindTarget { space: 0, register: 2, ..Default::default() }),

                (naga::ResourceBinding { group: 0, binding: 5 }, naga::back::hlsl::BindTarget { space: 0, register: 1, ..Default::default() }), // samplers
                (naga::ResourceBinding { group: 0, binding: 6 }, naga::back::hlsl::BindTarget { space: 0, register: 0, ..Default::default() }), // samplers
                (naga::ResourceBinding { group: 0, binding: 7 }, naga::back::hlsl::BindTarget { space: 0, register: 2, ..Default::default() }), // samplers
            ]),
            fake_missing_bindings: false,
            special_constants_binding: None,
            push_constants_target: None,
            zero_initialize_workgroup_memory: false,
        };
        let mut code = String::new();
        naga::back::hlsl::Writer::new(&mut code, &_options).write(&module, &info).unwrap();

        std::fs::write(frag_out_path.replace(".frag", ".hlsl"), &code).unwrap();
    }*/
    // Emit WGSL
    /*{
        let module = spv::parse_u8_slice(&main_shader, &in_spv_options).unwrap();
        let info = Validator::new(ValidationFlags::default(), Capabilities::all()).validate(&module).unwrap_pretty();

        let wgsl = naga::back::wgsl::write_string(&module, &info, naga::back::wgsl::WriterFlags::empty()).unwrap();

        std::fs::write(wgsl_out_path, &wgsl).unwrap();
        println!("{}", wgsl);
    }*/
    // Emit GLSL
    {
        let module = spv::parse_u8_slice(&glsl_shader, &in_spv_options).unwrap();
        let info = Validator::new(ValidationFlags::default(), Capabilities::all()).validate(&module).unwrap_pretty();

        let mut buffer = String::new();
        let options = glsl::Options {
            version: glsl::Version::Desktop(420),
            // writer_flags: glsl::WriterFlags::ADJUST_COORDINATE_SPACE,
            binding_map: glsl::BindingMap::from([
                (naga::ResourceBinding { group: 0, binding: 1 }, 1),
                (naga::ResourceBinding { group: 0, binding: 2 }, 2),
                (naga::ResourceBinding { group: 0, binding: 3 }, 3),
                (naga::ResourceBinding { group: 0, binding: 4 }, 4),
            ]),
            ..Default::default()
        };
        let pipeline_options = glsl::PipelineOptions {
            entry_point: "undistort_fragment".into(),
            shader_stage: naga::ShaderStage::Fragment,
            multiview: None,
        };

        let mut constants = naga::back::PipelineConstants::default();
        constants.insert("100".to_owned(), 2.0); // interpolation
        constants.insert("101".to_owned(), 1.0); // distortion_model
        constants.insert("102".to_owned(), 0.0); // digital_distortion_model
        constants.insert("103".to_owned(), 0.0); // flags
        let (module, info) = naga::back::pipeline_constants::process_overrides(&module, &info, &constants).unwrap();

        let policies = naga::proc::BoundsCheckPolicies {
            index:         naga::proc::BoundsCheckPolicy::Unchecked,
            buffer:        naga::proc::BoundsCheckPolicy::Unchecked,
            image_load:    naga::proc::BoundsCheckPolicy::Unchecked,
            binding_array: naga::proc::BoundsCheckPolicy::Unchecked,
        };


        /*let spvoptions = naga::back::spv::Options {
            lang_version: (1, 0),
            flags: naga::back::spv::WriterFlags::ADJUST_COORDINATE_SPACE
                 | naga::back::spv::WriterFlags::LABEL_VARYINGS
                 | naga::back::spv::WriterFlags::CLAMP_FRAG_DEPTH,
            binding_map: Default::default(),
            capabilities: None,
            bounds_check_policies: policies,
            zero_initialize_workgroup_memory: naga::back::spv::ZeroInitializeWorkgroupMemoryMode::Polyfill,
            debug_info: None,
        };
        let mut writer = naga::back::spv::Writer::new(&spvoptions).unwrap();
        let mut spv_buffer = Vec::new();
        writer.write(&module, &info, None, &None, &mut spv_buffer).unwrap();
        let bytes = spv_buffer.iter().fold(Vec::with_capacity(spv_buffer.len() * 4), |mut v, w| { v.extend_from_slice(&w.to_le_bytes()); v });
        let tmp = format!("{}/../compiled/stabilize.spv.temp", env!("CARGO_MANIFEST_DIR"));
        std::fs::write(&tmp, bytes).unwrap();

        let _ = std::process::Command::new("spirv-opt")
            .arg("-O")
            .arg("--ccp")
            .arg("--cfg-cleanup")
            .arg("--eliminate-dead-branches")
            .arg("--eliminate-dead-code-aggressive")
            .arg("--eliminate-dead-const")
            .arg("--eliminate-dead-functions")
            .arg("--if-conversion")

            .arg(&tmp)
            .args(["-o", &format!("{tmp}-opt")])
            .status().unwrap().success();

        let module = spv::parse_u8_slice(&std::fs::read(format!("{tmp}-opt")).unwrap(), &in_spv_options).unwrap();
        let info = Validator::new(ValidationFlags::default(), Capabilities::all()).validate(&module).unwrap_pretty();*/

        let mut writer = glsl::Writer::new(&mut buffer, &module, &info, &options, &pipeline_options, policies).unwrap();
        writer.write().unwrap();

        // Uints are not supported in ES
        buffer = buffer.replace("uint", "int")
                       .replace("0u", "0")
                       .replace("1u", "1")
                       .replace("2u", "2")
                       .replace("3u", "3")
                       .replace("4u", "4")
                       .replace("5u", "5")
                       .replace("6u", "6")
                       .replace("7u", "7")
                       .replace("8u", "8")
                       .replace("9u", "9");

        // Remove nested member
        let re = regex::Regex::new(r"struct (type_\d+) \{\s+(type_\d+) member;\s+\};").unwrap();
        for _ in 0..2 {
            for cap in re.captures_iter(&buffer.clone()) {
                let (org, [type1, type2]) = cap.extract();
                if buffer.contains(&format!("{type1} _group_0_binding_2_fs")) {
                    let org = org.to_owned();
                    buffer = buffer.replace(&format!("{type1} _group_0_binding_2_fs"), &format!("{type2} _group_0_binding_2_fs"));
                    buffer = buffer.replace("_group_0_binding_2_fs.member", "_group_0_binding_2_fs");
                    buffer = buffer.replace(&org, "");
                }
            }
        }

        std::fs::write(&frag_out_path, &buffer).unwrap();
    }
    // let qsb_out_path = format!("{}/../compiled/stabilize-{}-{}-{}.frag.qsb", env!("CARGO_MANIFEST_DIR"), distortion_model as u32, digital_distortion_model as u32, flags as u32);

    let _ = std::process::Command::new("../../../../ext/6.7.3/msvc2019_64/bin/qsb.exe")
            .args(["--glsl", "120,300 es,310 es,320 es,310,320,330,400,410,420"])
            //.args(["--glsl", "120,300 es,310 es,320 es,310"])
            .args(["--hlsl", "50"])
            .args(["--msl", "12"])
            .arg("-O")
            .args(["-o", &qsb_out_path])
            .arg(&frag_out_path)
            .status().unwrap().success();

    std::fs::remove_file(frag_out_path).unwrap();
}
