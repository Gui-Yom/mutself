use std::fs;

use proc_macro2::{Literal, TokenStream};
use quote::{format_ident, quote};
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{
    parse_macro_input, Expr, ExprArray, ExprLit, ExprMacro, ExprMethodCall, Ident, Lit, LitStr,
    Macro, Token, Visibility,
};

#[derive(Debug)]
struct StaticEntry {
    visibility: Visibility,
    name: Ident,
    init: TokenStream,
    len: usize,
}

fn prim_size(ty: &str) -> usize {
    match ty {
        "usize" => 8,
        _ => unreachable!(),
    }
}

impl Parse for StaticEntry {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let visibility: Visibility = input.parse()?;
        let name: Ident = input.parse()?;
        input.parse::<Token![=]>()?;
        let mut init: Expr = input.parse()?;
        input.parse::<Token![;]>()?;

        let mut len = 0;
        let mut new_init = quote! {
            #init
        };

        match &init {
            Expr::Lit(ExprLit { lit, attrs }) => match lit {
                Lit::Str(_) => {}
                Lit::ByteStr(_) => {}
                Lit::Byte(_) => {}
                Lit::Char(_) => {}
                Lit::Int(i) => {
                    new_init = quote! {
                        #i.to_ne_bytes()
                    };
                    len = prim_size(i.suffix());
                }
                Lit::Float(_) => {}
                Lit::Bool(_) => {}
                Lit::Verbatim(_) => {}
                _ => {}
            },
            Expr::Array(ExprArray {
                attrs,
                elems,
                bracket_token,
            }) => {}
            Expr::MethodCall(ExprMethodCall { method, .. }) => {
                dbg!(method);
            }
            Expr::Macro(ExprMacro {
                mac: Macro { path, tokens, .. },
                ..
            }) => {
                if path.segments.first().unwrap().ident == format_ident!("include_bytes") {
                    let str = syn::parse2::<LitStr>(tokens.clone()).unwrap().value();
                    let data = fs::read(&str).unwrap();
                    len = data.len();
                    let values = data.iter();
                    new_init = quote! {
                        [#(#values),*]
                    };
                }
            }
            _ => {}
        }

        Ok(Self {
            visibility,
            name,
            init: new_init,
            len,
        })
    }
}

#[derive(Debug)]
struct MacroInput {
    entries: Vec<StaticEntry>,
}

impl Parse for MacroInput {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut entries = Vec::new();
        loop {
            match input.parse::<StaticEntry>() {
                Ok(e) => {
                    //eprintln!("{:#?}", e);
                    entries.push(e)
                }
                Err(e) => break,
            }
        }
        Ok(Self { entries })
    }
}

#[proc_macro]
pub fn mutself(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input = parse_macro_input!(input as MacroInput);

    proc_macro::TokenStream::from(if !input.entries.is_empty() {
        mutself_impl(input)
    } else {
        quote! {
            compile_error!("Expected at least one entry");
        }
    })
}

fn mutself_impl(input: MacroInput) -> TokenStream {
    let first = input
        .entries
        .first()
        .map(|e| format_ident!("{}_LEN", e.name))
        .unwrap();

    let section_name = ".mutself";
    let statics = input.entries.iter().enumerate().map(|(i, e)| {
        let ty_name = &e.name;
        let static_ident = format_ident!("{}_", &ty_name);
        let len_ident = format_ident!("{}LEN", &static_ident);

        let expr = &e.init;
        let len: usize = e.len;

        quote! {
            #[link_section = #section_name]
            static #len_ident: usize = #len;

            #[allow(unused)]
            #[link_section = #section_name]
            static #static_ident: [u8; #len] = #expr;

            #[allow(non_camel_case_types)]
            #[derive(Copy, Clone)]
            pub(super) struct #ty_name {
                __priv: (),
            }

            impl Deref for #ty_name {
                type Target = [u8];

                fn deref(&self) -> &Self::Target {
                    item::<#i>()
                }
            }

            pub(super) const #ty_name: #ty_name = #ty_name { __priv: () };
        }
    });

    let uses = input.entries.iter().map(|e| {
        let ty_name = &e.name;
        quote! {
            use __mutself::#ty_name;
        }
    });

    let args_decl = input.entries.iter().map(|e| {
        let name = format_ident!("{}", e.name.to_string().to_lowercase());
        quote! {
            #name: Option<&[u8]>
        }
    });
    let args_decl2 = args_decl.clone();

    let args = input
        .entries
        .iter()
        .map(|e| format_ident!("{}", e.name.to_string().to_lowercase()));
    let args2 = args.clone();

    let writers = input.entries.iter().map(|e| {
        let name = &e.name;
        let arg_name = format_ident!("{}", e.name.to_string().to_lowercase());
        let new_name = format_ident!("new_{}", &arg_name);
        let new_name_len = format_ident!("new_{}_len", &arg_name);
        quote! {
            let (#new_name, #new_name_len) = #arg_name
                .map(|d| (d, d.len()))
                .unwrap_or((&*#name, #name.len()));
            new_section.extend_from_slice(&#new_name_len.to_ne_bytes());
            new_section.extend(with_align(#new_name));
        }
    });

    let total_entries = input.entries.len();
    let section_name = Literal::byte_string(b".mutself");

    quote! {
        mod __mutself {
            use std::mem::size_of;
            use std::ops::Deref;
            use std::path::Path;
            use std::slice::from_raw_parts;
            use std::str::from_utf8_unchecked;
            use std::{env, fs};

            use object::pe::IMAGE_DIRECTORY_ENTRY_EXCEPTION;

            #[inline(always)]
            unsafe fn align(ptr: *const u8) -> *const u8 {
                ptr.add(ptr.align_offset(8))
            }

            #[inline(always)]
            fn item<const ITEM: usize>() -> &'static [u8] {
                assert!(ITEM < #total_entries);
                unsafe {
                    let mut ptr = &#first as *const usize as *const u8;
                    let mut size = *(ptr as *const usize);
                    for _ in 0..ITEM {
                        ptr = align(ptr.add(size_of::<usize>()).add(size));
                        size = *(ptr as *const usize);
                    }
                    from_raw_parts(ptr.add(size_of::<usize>()), size)
                }
            }

            #(#statics)*

            #[cfg(windows)]
            pub(super) fn mutself<P: AsRef<Path>>(
                new: P,
                #(#args_decl),*
            ) -> object::Result<()> {
                use object::pe::{
                    ImageDataDirectory, ImageNtHeaders32, ImageNtHeaders64,
                    IMAGE_DIRECTORY_ENTRY_BASERELOC, IMAGE_DIRECTORY_ENTRY_SECURITY,
                };
                use object::pe::{IMAGE_SCN_CNT_INITIALIZED_DATA, IMAGE_SCN_MEM_READ};
                use object::read::pe::{ImageNtHeaders, ImageOptionalHeader};
                use object::write::pe::NtHeaders;
                use object::LittleEndian as LE;
                use object::{FileKind, Object, ObjectSection};

                fn do_generic<Pe: ImageNtHeaders, P: AsRef<Path>>(
                    data: &[u8],
                    new: P,
                    #(#args_decl2),*
                ) -> object::Result<()> {
                    let exe = object::read::pe::PeFile::<Pe>::parse(&*data)?;

                    let file_header = exe.nt_headers().file_header();
                    let opt_header = exe.nt_headers().optional_header();

                    // for s in exe.sections() {
                    //     println!("{:?}", s.name());
                    // }

                    #[inline(always)]
                    fn with_align(data: &[u8]) -> impl Iterator<Item = u8> + '_ {
                        const ALIGN: usize = 8;
                        let masked = data.len() & (usize::MAX - ALIGN + 1);
                        let to_add = if masked == data.len() { 0 } else { ALIGN };

                        data.into_iter()
                            .map(|&v| v)
                            .chain(std::iter::repeat(0).take(masked + to_add - data.len()))
                    }

                    let mut new_section = Vec::new();
                    #(#writers)*

                    let mut out = Vec::with_capacity(data.len());
                    let mut writer = object::write::pe::Writer::new(
                        exe.is_64(),
                        opt_header.section_alignment(),
                        opt_header.file_alignment(),
                        &mut out,
                    );

                    writer.reserve_dos_header_and_stub();
                    if let Some(rich_header) = exe.rich_header_info() {
                        writer.reserve(rich_header.length as u32 + 8, 4);
                    }
                    writer.reserve_nt_headers(exe.data_directories().len());

                    let cert_dir = exe
                        .data_directory(IMAGE_DIRECTORY_ENTRY_SECURITY)
                        .map(ImageDataDirectory::address_range);
                    let reloc_dir = exe
                        .data_directory(IMAGE_DIRECTORY_ENTRY_BASERELOC)
                        .map(ImageDataDirectory::address_range);
                    for (i, dir) in exe.data_directories().iter().enumerate() {
                        if dir.virtual_address.get(LE) == 0
                            || i == IMAGE_DIRECTORY_ENTRY_SECURITY
                            || i == IMAGE_DIRECTORY_ENTRY_BASERELOC
                            || i == IMAGE_DIRECTORY_ENTRY_EXCEPTION
                        {
                            continue;
                        }
                        writer.set_data_directory(i, dir.virtual_address.get(LE), dir.size.get(LE));
                    }

                    writer.reserve_section_headers(exe.section_table().len() as u16);
                    let text_section = exe.section_by_name(".text").unwrap();
                    let text_range = writer.reserve_text_section(text_section.size() as u32);
                    let rdata_section = exe.section_by_name(".rdata").unwrap();
                    let rdata_range = writer.reserve_rdata_section(rdata_section.size() as u32);
                    let data_section = exe.section_by_name(".data").unwrap();
                    let data_range =
                        writer.reserve_data_section(data_section.size() as u32, data_section.size() as u32);
                    let pdata_section = exe.section_by_name(".pdata").unwrap();
                    let pdata_range = writer.reserve_pdata_section(pdata_section.size() as u32);
                    let custom_range = writer.reserve_section(
                        *#section_name,
                        IMAGE_SCN_CNT_INITIALIZED_DATA | IMAGE_SCN_MEM_READ,
                        new_section.len() as u32,
                        new_section.len() as u32,
                    );

                    if reloc_dir.is_some() {
                        let mut blocks = exe
                            .data_directories()
                            .relocation_blocks(data, &exe.section_table())?
                            .unwrap();
                        while let Some(block) = blocks.next()? {
                            for reloc in block {
                                writer.add_reloc(reloc.virtual_address, reloc.typ);
                            }
                        }
                        writer.reserve_reloc_section();
                    }

                    if let Some((_, size)) = cert_dir {
                        // TODO: reserve individual certificates
                        writer.reserve_certificate_table(size);
                    }

                    writer.write_dos_header_and_stub().unwrap();
                    if let Some(rich_header) = exe.rich_header_info() {
                        writer.write_align(4);
                        writer.write(&data[rich_header.offset..][..rich_header.length + 8]);
                    }

                    writer.write_nt_headers(NtHeaders {
                        machine: file_header.machine.get(LE),
                        time_date_stamp: file_header.time_date_stamp.get(LE),
                        characteristics: file_header.characteristics.get(LE),
                        major_linker_version: opt_header.major_linker_version(),
                        minor_linker_version: opt_header.minor_linker_version(),
                        address_of_entry_point: opt_header.address_of_entry_point(),
                        image_base: opt_header.image_base(),
                        major_operating_system_version: opt_header.major_operating_system_version(),
                        minor_operating_system_version: opt_header.minor_operating_system_version(),
                        major_image_version: opt_header.major_image_version(),
                        minor_image_version: opt_header.minor_image_version(),
                        major_subsystem_version: opt_header.major_subsystem_version(),
                        minor_subsystem_version: opt_header.minor_subsystem_version(),
                        subsystem: opt_header.subsystem(),
                        dll_characteristics: opt_header.dll_characteristics(),
                        size_of_stack_reserve: opt_header.size_of_stack_reserve(),
                        size_of_stack_commit: opt_header.size_of_stack_commit(),
                        size_of_heap_reserve: opt_header.size_of_heap_reserve(),
                        size_of_heap_commit: opt_header.size_of_heap_commit(),
                    });
                    writer.write_section_headers();
                    writer.write_section(text_range.file_offset, text_section.data()?);
                    writer.write_section(rdata_range.file_offset, rdata_section.data()?);
                    writer.write_section(data_range.file_offset, data_section.data()?);
                    writer.write_section(pdata_range.file_offset, pdata_section.data()?);
                    writer.write_section(custom_range.file_offset, &new_section);
                    writer.write_reloc_section();
                    if let Some((address, size)) = cert_dir {
                        // TODO: write individual certificates
                        writer.write_certificate_table(&data[address as usize..][..size as usize]);
                    }

                    fs::write(new, out).unwrap();

                    Ok(())
                }

                let data = fs::read(env::current_exe().unwrap()).unwrap();
                match FileKind::parse(&*data).unwrap() {
                    FileKind::Pe64 => {
                        do_generic::<ImageNtHeaders64, P>(&data, new, #(#args),*)?
                    }
                    FileKind::Pe32 => {
                        do_generic::<ImageNtHeaders32, P>(&data, new, #(#args2),*)?
                    }
                    _ => unreachable!(),
                }

                Ok(())
            }
        }

        use __mutself::mutself;
        #(#uses)*
    }
}
