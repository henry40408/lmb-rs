// https://github.com/EmbarkStudios/rust-ecosystem/blob/732513edfd9172f4eda358b2d0cefc6cad1585ee/lints.rs
#![deny(unsafe_code)]
#![warn(
    clippy::all,
    clippy::await_holding_lock,
    clippy::char_lit_as_u8,
    clippy::checked_conversions,
    clippy::dbg_macro,
    clippy::debug_assert_with_mut_call,
    clippy::doc_markdown,
    clippy::empty_enum,
    clippy::enum_glob_use,
    clippy::exit,
    clippy::expl_impl_clone_on_copy,
    clippy::explicit_deref_methods,
    clippy::explicit_into_iter_loop,
    clippy::fallible_impl_from,
    clippy::filter_map_next,
    clippy::flat_map_option,
    clippy::float_cmp_const,
    clippy::fn_params_excessive_bools,
    clippy::from_iter_instead_of_collect,
    clippy::if_let_mutex,
    clippy::implicit_clone,
    clippy::imprecise_flops,
    clippy::inefficient_to_string,
    clippy::invalid_upcast_comparisons,
    clippy::large_digit_groups,
    clippy::large_stack_arrays,
    clippy::large_types_passed_by_value,
    clippy::let_unit_value,
    clippy::linkedlist,
    clippy::lossy_float_literal,
    clippy::macro_use_imports,
    clippy::manual_ok_or,
    clippy::map_err_ignore,
    clippy::map_flatten,
    clippy::map_unwrap_or,
    clippy::match_on_vec_items,
    clippy::match_same_arms,
    clippy::match_wild_err_arm,
    clippy::match_wildcard_for_single_variants,
    clippy::mem_forget,
    clippy::mismatched_target_os,
    clippy::missing_enforced_import_renames,
    clippy::mut_mut,
    clippy::mutex_integer,
    clippy::needless_borrow,
    clippy::needless_continue,
    clippy::needless_for_each,
    clippy::option_option,
    clippy::path_buf_push_overwrite,
    clippy::ptr_as_ptr,
    clippy::rc_mutex,
    clippy::ref_option_ref,
    clippy::rest_pat_in_fully_bound_structs,
    clippy::same_functions_in_if_condition,
    clippy::semicolon_if_nothing_returned,
    clippy::single_match_else,
    clippy::string_add,
    clippy::string_add_assign,
    clippy::string_lit_as_bytes,
    clippy::string_to_string,
    clippy::todo,
    clippy::trait_duplication_in_bounds,
    clippy::unimplemented,
    clippy::unnested_or_patterns,
    clippy::unused_self,
    clippy::useless_transmute,
    clippy::verbose_file_reads,
    clippy::zero_sized_map_values,
    future_incompatible,
    nonstandard_style,
    rust_2018_idioms
)]

use std::time::Instant;

use mlua::{Lua, Table, ThreadStatus, VmState};
use thiserror::Error;

const DEFAULT_TIMEOUT: u64 = 30;
const K_LOADED: &str = "_LOADED";

#[derive(Debug, Error)]
pub enum LamError {
    #[error("lua error: {0}")]
    Lua(#[from] mlua::Error),
}

type LamResult<T> = Result<T, LamError>;

#[derive(Debug)]
pub struct Evaluation {
    pub script: String,
    pub timeout: Option<u64>,
}

fn register_lam_module(vm: &Lua) -> LamResult<()> {
    let loaded = vm.named_registry_value::<Table<'_>>(K_LOADED)?;

    let m = vm.create_table()?;
    m.set("_VERSION", env!("CARGO_PKG_VERSION"))?;
    loaded.set("@lam", m)?;

    vm.set_named_registry_value(K_LOADED, loaded)?;
    Ok(())
}

pub fn evaluate(e: &Evaluation) -> LamResult<String> {
    let start = Instant::now();
    let timeout = e.timeout.unwrap_or(DEFAULT_TIMEOUT) as f32;

    let vm = Lua::new();
    vm.sandbox(true)?;
    vm.set_interrupt(move |_| {
        if start.elapsed().as_secs_f32() > timeout {
            return Ok(VmState::Yield);
        }
        Ok(VmState::Continue)
    });
    register_lam_module(&vm)?;

    let co = vm.create_thread(vm.load(&e.script).into_function()?)?;
    loop {
        let res = co.resume::<_, Option<String>>(())?;
        if co.status() != ThreadStatus::Resumable || start.elapsed().as_secs_f32() > timeout {
            return Ok(res.unwrap_or(String::new()));
        }
    }
}

#[cfg(test)]
mod test {
    use std::time::Instant;

    use crate::{evaluate, Evaluation};

    #[test]
    fn test_evaluate_infinite_loop() {
        let timeout = 1;

        let start = Instant::now();
        let e = Evaluation {
            script: r#"while true do end"#.to_string(),
            timeout: Some(timeout),
        };
        let res = evaluate(&e).unwrap();
        assert_eq!("", res);

        let s = start.elapsed().as_secs_f32();
        let timeout = timeout as f32;
        assert!(s < timeout * 1.01, "timed out {}s", s);
    }
}
