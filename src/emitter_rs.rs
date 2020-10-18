#![allow(unused)]

use super::ast;
use super::flatten;
use super::make;
use super::emitter_common;
use super::name::Name;
use super::parser::{self, emit_error};
use super::project::Project;
use super::project;
use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::PathBuf;

pub struct Emitter {
    p: String,
    f: fs::File,
    module: flatten::Module,
    inside_macro: bool,
    cur_loc: Option<ast::Location>,
    closure_types:   HashSet<String>,
}

pub fn outname(_project: &Project, stage: &make::Stage, module: &flatten::Module) -> String {
    let td = project::target_dir();
    td.join("rust").join(format!("{}.rs", module.name.0[1..].join("_"))).to_string_lossy().to_string()
}

pub fn make_module(make: &super::make::Make) {
    let td      = project::target_dir();
    let pdir_   = td.join("rust").join(&make.artifact.name);
    let pdir    = std::path::Path::new(&pdir_);
    std::fs::create_dir_all(&pdir).unwrap();




    let p = pdir.join("build.rs");
    let mut f = fs::File::create(&p).expect(&format!("cannot create {:?}", p));

    write!(f, "fn main() {{\n").unwrap();
    write!(f, "    cc::Build::new()\n").unwrap();

    for step in &make.steps {
        write!(f,"      .file(\"{}\")\n",
            emitter_common::path_rel(&pdir, &step.source).to_string_lossy().to_string()
        ).unwrap();
    }
    for flag in &make.cincludes {
        write!(f,"      .include(\"{}\")\n",
            emitter_common::path_rel(&pdir, flag).to_string_lossy().to_string()
        ).unwrap();
    }
    write!(f, "    .compile(\"{}\");\n", make.artifact.name).unwrap();
    write!(f, "}}\n").unwrap();



    let p = pdir.join("Cargo.toml");
    let mut f = fs::File::create(&p).expect(&format!("cannot create {:?}", p));
    write!(f, r#"[package]
    name = "{an}"
    version = "0.0.1"
[dependencies]
libc = "0.2"
[build-dependencies]
cc = "1"
"#,
        an = make.artifact.name
    ).unwrap();



    let p = pdir.join("src");
    std::fs::create_dir_all(&p).unwrap();
    let p = p.join("lib.rs");

    let mut f = fs::File::create(&p).expect(&format!("cannot create {:?}", p));

    for step in &make.steps {
        if step.source.parent().unwrap().file_name().unwrap() == "zz" {
            let mut mn = step
                .source
                .file_stem()
                .unwrap()
                .to_string_lossy()
                .to_string();
            write!(
                f,
                "#[path = \"../../{}.rs\"]\n",
                step.source.file_stem().unwrap().to_string_lossy()
            )
            .unwrap();
            write!(f, "pub mod {};\n\n", mn).unwrap();
        }
    }
}

impl Emitter {
    pub fn new(project: &Project, stage: make::Stage, module: flatten::Module) -> Self {
        std::fs::create_dir_all(format!("target/rust/")).unwrap();
        let p = outname(project, &stage, &module);
        let f = fs::File::create(&p).expect(&format!("cannot create {}", p));

        Emitter {
            p,
            f,
            module,
            inside_macro: false,
            cur_loc: None,
            closure_types: HashSet::new(),
        }
    }

    fn emit_loc(&mut self, loc: &ast::Location) {
        if let Some(cur_loc) = &self.cur_loc {
            if cur_loc.file == loc.file && cur_loc.line == loc.line {
                return;
            }
        }
        self.cur_loc = Some(loc.clone());
        //write!(self.f, "// line {} \"{}\"\n", loc.line(), loc.file).unwrap();
    }

    fn to_local_typed_name(&self, name: &ast::Typed) -> Option<String> {
        Some(match name.t {
            ast::Type::U8 => "u8".to_string(),
            ast::Type::U16 => "u16".to_string(),
            ast::Type::U32 => "u32".to_string(),
            ast::Type::U64 => "u64".to_string(),
            ast::Type::U128 => "u128".to_string(),
            ast::Type::I8 => "i8".to_string(),
            ast::Type::I16 => "i16".to_string(),
            ast::Type::I32 => "i32".to_string(),
            ast::Type::I64 => "i64".to_string(),
            ast::Type::I128 => "i128".to_string(),
            ast::Type::Int => "std::os::raw::c_int".to_string(),
            ast::Type::UInt => "std::os::raw::c_uint".to_string(),
            ast::Type::ISize => "isize".to_string(),
            ast::Type::USize => "usize".to_string(),
            ast::Type::Bool => "bool".to_string(),
            ast::Type::F32 => "f32".to_string(),
            ast::Type::F64 => "f64".to_string(),
            ast::Type::Char => "u8".to_string(),
            ast::Type::Void => "std::ffi::c_void".to_string(),
            ast::Type::Other(ref n) => {
                if name.ptr.len() != 1 {
                    if n.0[1] == "ext" {
                        if n.0[3] == "char" {
                            "u8".to_string()
                        } else {
                            return None;
                        }
                    } else {
                        format!(
                            "super::{}::{}",
                            n.0[1..n.0.len() - 1].join("_"),
                            n.0.last().unwrap()
                        )
                    }
                } else {
                    "u8".to_string()
                }
            }
            /*
                let mut s = self.to_local_name(&n);
                match &name.tail {
                    ast::Tail::Dynamic | ast::Tail::None | ast::Tail::Bind(_,_)=> {},
                    ast::Tail::Static(v,_) => {
                        s = format!("{}_{}", s, v);
                    }
                }
                s
            }
                */
            ast::Type::ILiteral | ast::Type::ULiteral | ast::Type::Elided | ast::Type::New | ast::Type::Typeid=> {
                parser::emit_error(
                    "ICE: untyped ended up in emitter",
                    &[(
                        name.loc.clone(),
                        format!("this should have been resolved earlier"),
                    )],
                );
                std::process::exit(9);
            }
        })
    }
    fn to_local_name(&self, s: &Name) -> String {
        if !s.is_absolute() {
            return s.0.join("_");
        }

        assert!(s.is_absolute(), "ICE not abs: '{}'", s);
        if let Some(an) = self.module.aliases.get(&s) {
            return an.clone();
        }

        if s.0[1] == "ext" {
            return s.0.last().unwrap().clone();
        }

        let mut s = s.clone();
        s.0.remove(0);
        return s.0.join("_");
    }

    pub fn emit(mut self) {
        let module = self.module.clone();
        debug!("emitting rs {}", module.name);

        write!(
            self.f,
            "#![allow(non_camel_case_types)]\n#![allow(dead_code)]\n"
        )
        .unwrap();
        write!(self.f, "extern crate libc;\n").unwrap();

        for (d, complete) in &module.d {
            let mut dmodname = Name::from(&d.name);
            dmodname.pop();
            if dmodname != module.name {
                continue;
            }
            if complete != &flatten::TypeComplete::Complete {
                continue;
            }
            self.emit_loc(&d.loc);
            match d.def {
                ast::Def::Struct { .. } => {
                    self.emit_struct_stack(&d, None);
                    if let Some(vs) = module.typevariants.get(&Name::from(&d.name)) {
                        for (v, tvloc) in vs {
                            let mut d = d.clone();
                            d.name = format!("{}_{}", d.name, v);
                            self.emit_struct_stack(&d, Some(*v));
                        }
                    }
                }
                ast::Def::Enum { .. } => self.emit_enum(&d),
                ast::Def::Closure { .. } => {
                    self.emit_closure(&d);
                }
                ast::Def::Const { .. } => self.emit_const(&d),
                _ => (),
            }
        }

        write!(self.f, "\npub mod heap {{\n").unwrap();
        for (d, complete) in &module.d {
            let mut dmodname = Name::from(&d.name);
            dmodname.pop();
            if dmodname != module.name {
                continue;
            }
            if complete != &flatten::TypeComplete::Complete {
                continue;
            }
            match d.def {
                ast::Def::Struct { .. } => {
                    self.emit_struct_heap(&d, None);
                    if let Some(vs) = module.typevariants.get(&Name::from(&d.name)) {
                        for (v, tvloc) in vs {
                            let mut d = d.clone();
                            d.name = format!("{}_{}", d.name, v);
                            self.emit_struct_heap(&d, Some(*v));
                        }
                    }
                }
                _ => (),
            }
        }
        write!(self.f, "}}\n").unwrap();

        write!(self.f, "extern {{\n").unwrap();

        for (d, complete) in &module.d {
            let mut dmodname = Name::from(&d.name);
            dmodname.pop();
            if dmodname != module.name {
                continue;
            }
            if complete != &flatten::TypeComplete::Complete {
                continue;
            }

            let mut dmodname = Name::from(&d.name);
            dmodname.pop();
            if dmodname != module.name {
                continue;
            }

            self.emit_loc(&d.loc);
            match d.def {
                ast::Def::Macro { .. } => {}
                ast::Def::Static { .. } => self.emit_static(&d),
                ast::Def::Struct { .. } => {
                    self.emit_struct_len(&d, None);

                    if let Some(vs) = module.typevariants.get(&Name::from(&d.name)) {
                        for (v, _) in vs {
                            let mut d = d.clone();
                            d.name = format!("{}_{}", d.name, v);
                            self.emit_struct_len(&d, Some(*v));
                        }
                    }
                }
                ast::Def::Function { .. } => {
                    if !d.name.ends_with("::main") {
                        self.emit_decl(&d);
                    }
                }
                _ => {}
            }
            write!(self.f, "\n").unwrap();
        }

        write!(self.f, "}}\n").unwrap();
    }

    pub fn emit_static(&mut self, ast: &ast::Local) {
        self.emit_loc(&ast.loc);
        let (_typed, _expr, _tags, storage, _array) = match &ast.def {
            ast::Def::Static {
                typed,
                expr,
                tags,
                storage,
                array,
            } => (typed, expr, tags, storage, array),
            _ => unreachable!(),
        };

        match storage {
            ast::Storage::Atomic => {
                return;
            }
            ast::Storage::ThreadLocal => {
                return;
            }
            ast::Storage::Static => (),
        }
    }

    pub fn emit_const(&mut self, ast: &ast::Local) {
        self.emit_loc(&ast.loc);
        let (typed, expr) = match &ast.def {
            ast::Def::Const { typed, expr } => (typed, expr),
            _ => unreachable!(),
        };

        let shortname = Name::from(&ast.name).0.last().unwrap().clone();
        let fieldtype = match self.to_local_typed_name(&typed) {
            Some(v) => v,
            None => {
                return;
            }
        };

        let fieldval = match expr {
            ast::Expression::LiteralChar { v, .. } => format!("'{}'", v),
            ast::Expression::Literal { v, .. } => format!("{}", v),
            _ => {
                return;
            }
        };

        write!(self.f, "pub const {} : ", shortname).unwrap();
        self.emit_pointer(&typed.ptr);
        write!(self.f, "{} = {};\n", fieldtype, fieldval).unwrap();
    }

    pub fn emit_enum(&mut self, ast: &ast::Local) {
        self.emit_loc(&ast.loc);
        let names = match &ast.def {
            ast::Def::Enum { names, .. } => (names),
            _ => unreachable!(),
        };
        let shortname = Name::from(&ast.name).0.last().unwrap().clone();
        write!(self.f, "#[derive(Copy,Clone, PartialEq)]\n").unwrap();
        write!(self.f, "#[repr(C)]\n").unwrap();
        write!(self.f, "pub enum {} {{\n", shortname).unwrap();
        for (name, literal) in names {
            write!(
                self.f,
                "    {}_{}",
                self.to_local_name(&Name::from(&ast.name)),
                name
            )
            .unwrap();
            if let Some(literal) = literal {
                write!(self.f, " = {}", literal).unwrap();
            }
            write!(self.f, ",\n").unwrap();
        }
        write!(self.f, "\n}}\n\n").unwrap();
    }

    pub fn emit_struct_len(&mut self, ast: &ast::Local, tail_variant: Option<u64>) {
        let (_fields, _packed, tail, _union) = match &ast.def {
            ast::Def::Struct {
                fields,
                packed,
                tail,
                union,
                ..
            } => (fields, packed, tail, union),
            _ => unreachable!(),
        };
        let shortname = Name::from(&ast.name).0.last().unwrap().clone();

        if tail == &ast::Tail::None || tail_variant.is_some() {
            write!(
                self.f,
                "    #[link_name = \"sizeof_{}\"]\n",
                self.to_local_name(&Name::from(&ast.name))
            )
            .unwrap();
            write!(
                self.f,
                "    pub fn sizeof_{}() -> libc::size_t;\n",
                shortname
            )
            .unwrap();
        } else {
            write!(
                self.f,
                "    #[link_name = \"sizeof_{}\"]\n",
                self.to_local_name(&Name::from(&ast.name))
            )
            .unwrap();
            write!(
                self.f,
                "    pub fn sizeof_{}(tail: libc::size_t) -> libc::size_t;\n",
                shortname
            )
            .unwrap();
        }
    }

    pub fn emit_struct_heap(&mut self, ast: &ast::Local, tail_variant: Option<u64>) {
        let (fields, _packed, tail, union) = match &ast.def {
            ast::Def::Struct {
                fields,
                packed,
                tail,
                union,
                ..
            } => (fields, packed, tail, *union),
            _ => unreachable!(),
        };
        let shortname = Name::from(&ast.name).0.last().unwrap().clone();

        // dont emit struct if we cant type all fields
        // TODO this actually sucks.
        if fields.len() > 1 {
            for field in &fields[..fields.len() - 1] {
                if self.to_local_typed_name(&field.typed).is_none() {
                    return;
                };
            }
        }

        // dont emit if its a union
        if union {return; }

        write!(
            self.f,
            r#"
pub struct {name} {{
    pub inner:  Box<super::{name}>,
    pub tail:   usize,
}}

impl std::ops::Deref for {name} {{
    type Target = super::{name};

    fn deref(&self) -> &super::{name} {{
        self.inner.deref()
    }}
}}

impl std::clone::Clone for {name} {{
    fn clone(&self) -> Self {{
        unsafe {{
"#,
            name = shortname
        )
        .unwrap();

        if tail == &ast::Tail::None || tail_variant.is_some() {
            write!(
                self.f,
                "            let size = super::sizeof_{name}();\n",
                name = shortname
            )
            .unwrap();
        } else {
            write!(
                self.f,
                "            let size = super::sizeof_{name}(self.tail);\n",
                name = shortname
            )
            .unwrap();
        }

        write!(
            self.f,
            r#"
            let mut s = Box::new(vec![0u8; size]);
            std::ptr::copy_nonoverlapping(self._self(), s.as_mut_ptr(), size);

            let ss : *mut super::{name} = std::mem::transmute(Box::leak(s).as_mut_ptr());

            Self {{ inner: Box::from_raw(ss), tail: self.tail }}
        }}
    }}
}}

impl {name} {{
    pub fn _tail(&mut self) -> usize {{
        self.tail
    }}
    pub fn _self_mut(&mut self) -> *mut u8 {{
        unsafe {{ std::mem::transmute(self.inner.as_mut() as *mut super::{name}) }}
    }}
    pub fn _self(&self) -> *const u8 {{
        unsafe {{ std::mem::transmute(self.inner.as_ref() as *const super::{name}) }}
    }}
}}

"#,
            name = shortname
        )
        .unwrap();

        write!(self.f, "impl {} {{\n", shortname).unwrap();

        if tail == &ast::Tail::None || tail_variant.is_some() {
            write!(self.f, "    pub fn new() -> Self {{\n").unwrap();
            write!(self.f, "        let tail = 0;\n").unwrap();
            write!(
                self.f,
                "        let size = unsafe{{super::sizeof_{}()}};\n",
                shortname
            )
            .unwrap();
        } else {
            write!(self.f, "    pub fn new(tail:  usize) -> Self {{\n").unwrap();
            write!(
                self.f,
                "        let size = unsafe{{super::sizeof_{}(tail)}};\n",
                shortname
            )
            .unwrap();
        }

        write!(self.f, "        unsafe {{\n").unwrap();
        write!(self.f, "            let s = Box::new(vec![0u8; size]);\n").unwrap();
        write!(
            self.f,
            "            let ss : *mut super::{} = std::mem::transmute(Box::leak(s).as_mut_ptr());\n",
            shortname
        )
        .unwrap();
        write!(
            self.f,
            "            Self {{ inner: Box::from_raw(ss), tail }} \n"
        )
        .unwrap();
        write!(self.f, "        }}\n").unwrap();
        write!(self.f, "    }}\n").unwrap();
        write!(self.f, "}}\n").unwrap();
    }

    pub fn emit_struct_stack(&mut self, ast: &ast::Local, tail_variant: Option<u64>) {
        let (fields, _packed, tail, union) = match &ast.def {
            ast::Def::Struct {
                fields,
                packed,
                tail,
                union,
                ..
            } => (fields, packed, tail, *union),
            _ => unreachable!(),
        };
        let shortname = Name::from(&ast.name).0.last().unwrap().clone();

        // dont emit struct if we cant type all fields
        // TODO this actually sucks.
        if fields.len() > 1 {
            for field in &fields[..fields.len() - 1] {
                if self.to_local_typed_name(&field.typed).is_none() {
                    return;
                };
            }
        }

        if union {
            write!(self.f, "\n#[derive(Copy,Clone)]\n#[repr(C)]\npub union {} {{\n", shortname).unwrap();
        } else {
            write!(self.f, "\n#[derive(Copy,Clone)]\n#[repr(C)]\npub struct {} {{\n", shortname).unwrap();
        }

        for i in 0..fields.len() {
            let field = &fields[i];

            let fieldtype = match self.to_local_typed_name(&field.typed) {
                Some(v) => v,
                None => {
                    write!(self.f, "    // {}\n", field.name).unwrap();
                    continue;
                }
            };
            match &field.array {
                ast::Array::Sized(expr) => {
                    write!(self.f, "    pub {} : [", field.name).unwrap();
                    self.emit_pointer(&field.typed.ptr);
                    write!(self.f, "{};", fieldtype).unwrap();
                    self.emit_expr(expr);
                    write!(self.f, "]").unwrap();
                }
                ast::Array::Unsized => {
                    if i != (fields.len() - 1) {
                        parser::emit_error(
                            "tail field has no be the last field in a struct",
                            &[(
                                field.loc.clone(),
                                format!("tail field would displace next field"),
                            )],
                        );
                        std::process::exit(9);
                    }
                    if let Some(tt) = tail_variant {
                        write!(self.f, "    pub {} : [", field.name).unwrap();
                        self.emit_pointer(&field.typed.ptr);
                        write!(self.f, "{};{}]", fieldtype, tt).unwrap();
                    } else {
                        // rust makes unsized types 128bit pointers which is incompatible with C ABI.
                        // nothing we can do
                        write!(self.f, "    // {}", field.name).unwrap();
                    }
                }
                ast::Array::None => {
                    write!(self.f, "    pub {} :", field.name).unwrap();
                    self.emit_pointer(&field.typed.ptr);
                    write!(self.f, "{}", fieldtype).unwrap();
                }
            }

            write!(self.f, " ,\n").unwrap();
        }
        write!(self.f, "}}\n").unwrap();
    }

    fn function_args(&mut self, args: &Vec<ast::NamedArg>) {
        let mut first = true;
        for arg in args {
            let argtype = match self.to_local_typed_name(&arg.typed) {
                Some(v) => v,
                None => continue,
            };
            if first {
                first = false;
            } else {
                write!(self.f, ", ").unwrap();
            }

            write!(self.f, " Z{}: ", arg.name).unwrap();

            self.emit_pointer(&arg.typed.ptr);
            write!(self.f, "{}", argtype).unwrap();
        }
    }

    pub fn emit_closure(&mut self, ast: &ast::Local) {
        let (ret, args, attr) = match &ast.def {
            ast::Def::Closure {
                ret,
                args,
                attr,
                ..
            } => (ret, args, attr),
            _ => unreachable!(),
        };
        self.emit_loc(&ast.loc);

        let shortname = Name::from(&ast.name).0.last().unwrap().clone();

        write!(self.f, "#[derive(Copy,Clone)]\n#[repr(C)]\npub struct {sn} {{\n    pub ctx: *mut std::ffi::c_void,\n",
               sn = shortname,
        ).unwrap();

        write!(self.f, "    pub f: extern fn (").unwrap();
        self.function_args(args);

        if args.len() > 0 {
            write!(self.f, ", ").unwrap();
        }
        write!(self.f, "ctx: *mut std::ffi::c_void").unwrap();

        match &ret {
            None => {
                write!(self.f, "),\n").unwrap();
            }
            Some(a) => {
                write!(self.f, ") -> {},\n", self.to_local_typed_name(&a.typed).unwrap()).unwrap();
                self.emit_pointer(&a.typed.ptr);
            }
        };

        write!(self.f, "}}\n").unwrap();

    }

    pub fn emit_decl(&mut self, ast: &ast::Local) {
        let (ret, args, _body, _vararg, _attr) = match &ast.def {
            ast::Def::Function {
                ret,
                args,
                body,
                vararg,
                attr,
                ..
            } => (ret, args, body, *vararg, attr),
            _ => unreachable!(),
        };

        let shortname = Name::from(&ast.name).0.last().unwrap().clone();
        let rettype = match &ret {
            None => None,
            Some(a) => match self.to_local_typed_name(&a.typed) {
                None => return,
                Some(v) => Some(v),
            },
        };

        write!(
            self.f,
            "    #[link_name = \"{}\"]\n",
            self.to_local_name(&Name::from(&ast.name))
        )
        .unwrap();
        write!(self.f, "    pub fn r#{}(", shortname).unwrap();

        self.function_args(args);
        write!(self.f, ")").unwrap();

        if let (Some(a), Some(rettype)) = (&ret, &rettype) {
            write!(self.f, "  -> ").unwrap();
            self.emit_pointer(&a.typed.ptr);
            write!(self.f, "{}", rettype).unwrap();
        };

        write!(self.f, ";\n").unwrap();
    }

    fn emit_pointer(&mut self, v: &Vec<ast::Pointer>) {
        for ptr in v {
            write!(self.f, "*").unwrap();
            if ptr.tags.contains_key("mut") || ptr.tags.contains_key("mut") {
                write!(self.f, "mut ").unwrap();
            } else {
                write!(self.f, "const ").unwrap();
            }
        }
    }

    fn emit_expr(&mut self, v: &ast::Expression) {
        match v {
            ast::Expression::Unsafe { expr, .. } => {
                self.emit_expr(expr);
            }
            ast::Expression::MacroCall { loc, .. } => {
                parser::emit_error(
                    "ICE: incomplete macro expansion ended up in emitter",
                    &[(
                        loc.clone(),
                        format!("this should have been resolved earlier"),
                    )],
                );
                std::process::exit(9);
            }
            ast::Expression::ArrayInit { .. } => {}
            ast::Expression::StructInit { .. } => {}
            ast::Expression::UnaryPost { expr, loc, op } => {
                write!(self.f, "(").unwrap();
                self.emit_loc(&loc);
                self.emit_expr(expr);
                write!(
                    self.f,
                    " {}",
                    match op {
                        ast::PostfixOperator::Increment => "++",
                        ast::PostfixOperator::Decrement => "--",
                    }
                )
                .unwrap();
                write!(self.f, ")").unwrap();
            }
            ast::Expression::UnaryPre { expr, loc, op } => {
                write!(self.f, "(").unwrap();
                self.emit_loc(&loc);
                write!(
                    self.f,
                    " {}",
                    match op {
                        ast::PrefixOperator::Boolnot => "!",
                        ast::PrefixOperator::Bitnot => "~",
                        ast::PrefixOperator::Increment => "++",
                        ast::PrefixOperator::Decrement => "--",
                        ast::PrefixOperator::AddressOf => "&",
                        ast::PrefixOperator::Deref => "*",
                    }
                )
                .unwrap();
                self.emit_expr(expr);
                write!(self.f, ")").unwrap();
            }
            ast::Expression::Cast { .. } => {}
            ast::Expression::Name(name) => {
                self.emit_loc(&name.loc);
                write!(
                    self.f,
                    "    {}",
                    self.to_local_typed_name(&name).unwrap_or("()".to_string())
                )
                .unwrap();
            }
            ast::Expression::LiteralString { loc, v } => {
                self.emit_loc(&loc);
                write!(self.f, "    \"").unwrap();
                for c in v {
                    self.write_escaped_literal(*c, true);
                }
                write!(self.f, "\"").unwrap();
            }
            ast::Expression::LiteralChar { loc, v } => {
                self.emit_loc(&loc);
                write!(self.f, "    '").unwrap();
                self.write_escaped_literal(*v, false);
                write!(self.f, "'").unwrap();
            }
            ast::Expression::Literal { loc, v } => {
                self.emit_loc(&loc);
                write!(self.f, "    {}", v).unwrap();
            }
            ast::Expression::Call {
                loc,
                name,
                args,
                emit,
                ..
            } => {
                match emit {
                    ast::EmitBehaviour::Default => {}
                    ast::EmitBehaviour::Skip => {
                        return;
                    }
                    ast::EmitBehaviour::Error { loc, message } => {
                        emit_error(format!("{}", message), &[(loc.clone(), "here")]);
                        std::process::exit(9);
                    }
                };

                self.emit_loc(&loc);
                self.emit_expr(&name);
                write!(self.f, "(").unwrap();

                let mut first = true;
                for arg in args {
                    if first {
                        first = false;
                    } else {
                        write!(self.f, ",").unwrap();
                    }
                    self.emit_expr(arg);
                }
                write!(self.f, "    )").unwrap();
            }
            ast::Expression::Infix {
                lhs, rhs, op, loc, ..
            } => {
                write!(self.f, "(").unwrap();
                self.emit_expr(lhs);
                self.emit_loc(&loc);
                write!(
                    self.f,
                    " {}",
                    match op {
                        ast::InfixOperator::Equals => "==",
                        ast::InfixOperator::Nequals => "!=",
                        ast::InfixOperator::Add => "+",
                        ast::InfixOperator::Subtract => "-",
                        ast::InfixOperator::Multiply => "*",
                        ast::InfixOperator::Divide => "/",
                        ast::InfixOperator::Bitxor => "^",
                        ast::InfixOperator::Booland => "&&",
                        ast::InfixOperator::Boolor => "||",
                        ast::InfixOperator::Moreeq => ">=",
                        ast::InfixOperator::Lesseq => "<=",
                        ast::InfixOperator::Lessthan => "<",
                        ast::InfixOperator::Morethan => ">",
                        ast::InfixOperator::Shiftleft => "<<",
                        ast::InfixOperator::Shiftright => ">>",
                        ast::InfixOperator::Modulo => "%",
                        ast::InfixOperator::Bitand => "&",
                        ast::InfixOperator::Bitor => "|",
                    }
                )
                .unwrap();
                self.emit_expr(rhs);
                write!(self.f, "  )").unwrap();
            }
            ast::Expression::MemberAccess { loc, lhs, rhs, op } => {
                self.emit_loc(&loc);
                self.emit_expr(lhs);
                write!(self.f, " {}{}", op, rhs).unwrap();
            }
            ast::Expression::ArrayAccess { loc, lhs, rhs } => {
                self.emit_loc(&loc);
                self.emit_expr(lhs);
                write!(self.f, " [ ").unwrap();
                self.emit_expr(rhs);
                write!(self.f, "]").unwrap();
            }
            ast::Expression::Cpp{loc, ..} => {
                parser::emit_error(
                    "invalid c preprocessor directive in local expression location".to_string(),
                    &[(
                        loc.clone(),
                        format!("c preprocessor expression not possible in this location"),
                    )],
                );
                std::process::exit(9);
            }
        }
    }

    fn write_escaped_literal(&mut self, c: u8, isstr: bool) {
        let c = c as char;
        match c {
            '"' if isstr => {
                write!(self.f, "\\\"").unwrap();
            }
            '\'' if !isstr => {
                write!(self.f, "\\'").unwrap();
            }
            '\\' => {
                write!(self.f, "\\\\").unwrap();
            }
            '\t' => {
                write!(self.f, "\\t").unwrap();
            }
            '\r' => {
                write!(self.f, "\\r").unwrap();
            }
            '\n' => {
                write!(self.f, "\\n").unwrap();
            }
            _ if c.is_ascii() && !c.is_ascii_control() => {
                write!(self.f, "{}", c).unwrap();
            }
            _ => {
                if isstr {
                    write!(self.f, "\"\"\\x{:x}\"\"", c as u8).unwrap();
                } else {
                    write!(self.f, "\\x{:x}", c as u8).unwrap();
                }
            }
        }
    }
}
