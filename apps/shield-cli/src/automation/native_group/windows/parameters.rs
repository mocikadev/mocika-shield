//! Windows 原生命令参数与完整环境块；只处理数据，不解析 shell 语法。
use crate::automation::native_group::CommandSpec;
use std::{cmp::Ordering, ffi::OsStr, io, os::windows::ffi::OsStrExt};
use windows_sys::Win32::Globalization::{
    CompareStringOrdinal, CSTR_EQUAL, CSTR_GREATER_THAN, CSTR_LESS_THAN,
};

pub(super) struct Parameters {
    pub program: Vec<u16>,
    pub arguments: Vec<u16>,
    pub directory: Vec<u16>,
    pub environment: Vec<u16>,
}

pub(super) fn wide(value: &OsStr) -> io::Result<Vec<u16>> {
    let units: Vec<_> = value.encode_wide().collect();
    if units.contains(&0) {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    Ok(units)
}

impl Parameters {
    pub fn new(spec: &CommandSpec) -> io::Result<Self> {
        if !spec.program.is_absolute()
            || !spec.directory.is_absolute()
            || !spec
                .program
                .extension()
                .is_some_and(|v| v.eq_ignore_ascii_case("exe"))
        {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        let program = wide(spec.program.as_os_str())?;
        if program.contains(&(b'"' as u16)) {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        let mut arguments = Vec::new();
        for value in std::iter::once(spec.program.as_os_str())
            .chain(spec.arguments.iter().map(|v| v.as_os_str()))
        {
            if !arguments.is_empty() {
                arguments.push(b' ' as u16);
            }
            quote(&wide(value)?, &mut arguments);
        }
        if arguments.len() >= 32767 {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        arguments.push(0);
        let mut directory = wide(spec.directory.as_os_str())?;
        directory.push(0);
        let mut program = program;
        program.push(0);
        Ok(Self {
            program,
            arguments,
            directory,
            environment: environment(spec)?,
        })
    }
}

fn quote(value: &[u16], output: &mut Vec<u16>) {
    // 原生 CRT／Rust argv 规则：引号前的反斜杠加倍，尾部反斜杠不得吞掉闭引号。
    output.push(b'"' as u16);
    let mut slashes = 0;
    for &unit in value {
        if unit == b'\\' as u16 {
            slashes += 1;
            continue;
        }
        let count = if unit == b'"' as u16 {
            slashes * 2 + 1
        } else {
            slashes
        };
        output.extend(std::iter::repeat_n(b'\\' as u16, count));
        output.push(unit);
        slashes = 0;
    }
    output.extend(std::iter::repeat_n(b'\\' as u16, slashes * 2));
    output.push(b'"' as u16);
}

fn compare(first: &[u16], second: &[u16]) -> io::Result<Ordering> {
    let first_len = i32::try_from(first.len()).map_err(|_| io::ErrorKind::InvalidInput)?;
    let second_len = i32::try_from(second.len()).map_err(|_| io::ErrorKind::InvalidInput)?;
    match unsafe { CompareStringOrdinal(first.as_ptr(), first_len, second.as_ptr(), second_len, 1) }
    {
        CSTR_LESS_THAN => Ok(Ordering::Less),
        CSTR_EQUAL => Ok(Ordering::Equal),
        CSTR_GREATER_THAN => Ok(Ordering::Greater),
        _ => Err(io::Error::last_os_error()),
    }
}

fn environment(spec: &CommandSpec) -> io::Result<Vec<u16>> {
    let mut values: Vec<(Vec<u16>, Vec<u16>)> = Vec::new();
    for (name, value) in &spec.environment {
        let name = wide(name)?;
        // 保留系统的 =C: 驱动器目录项，其他名称不允许等号。
        let drive = name.len() == 3
            && name[0] == b'=' as u16
            && name[2] == b':' as u16
            && u8::try_from(name[1]).is_ok_and(|v| v.is_ascii_alphabetic());
        if name.is_empty() || (!drive && name.contains(&(b'=' as u16))) {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        let value = wide(value)?;
        let mut index = 0;
        while index < values.len() {
            match compare(&name, &values[index].0)? {
                Ordering::Less => break,
                Ordering::Equal => return Err(io::ErrorKind::InvalidInput.into()),
                Ordering::Greater => index += 1,
            }
        }
        values.insert(index, (name, value));
    }
    let mut block = Vec::new();
    for (name, value) in values {
        block.extend(name);
        block.push(b'=' as u16);
        block.extend(value);
        block.push(0);
        if block.len() > 128 * 1024 {
            return Err(io::ErrorKind::InvalidInput.into());
        }
    }
    if block.is_empty() {
        block.push(0);
    }
    block.push(0);
    Ok(block)
}
