//! 仅用于研究报告的方法处置与指令覆盖率汇总。

use std::fmt;

use super::{DexInventory, MethodDisposition};

#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct DexResearchSummary {
    methods_total: usize,
    methods_with_code: usize,
    eligible_methods: usize,
    instruction_bytes: u64,
    eligible_instruction_bytes: u64,
    constructors: usize,
    static_initializers: usize,
    native_methods: usize,
    abstract_methods: usize,
    other_skipped: usize,
}

pub(super) fn summarize(inventory: &DexInventory) -> DexResearchSummary {
    let mut summary = DexResearchSummary {
        methods_total: inventory.methods.len(),
        ..DexResearchSummary::default()
    };
    for method in &inventory.methods {
        if let Some(code) = &method.code {
            summary.methods_with_code += 1;
            summary.instruction_bytes += u64::from(code.instructions_size) * 2;
            if method.disposition == MethodDisposition::Eligible {
                summary.eligible_instruction_bytes += u64::from(code.instructions_size) * 2;
            }
        }
        match method.disposition {
            MethodDisposition::Eligible => summary.eligible_methods += 1,
            MethodDisposition::Constructor => summary.constructors += 1,
            MethodDisposition::StaticInitializer => summary.static_initializers += 1,
            MethodDisposition::Native => summary.native_methods += 1,
            MethodDisposition::Abstract => summary.abstract_methods += 1,
            MethodDisposition::MissingCode
            | MethodDisposition::InstructionSpaceTooSmall
            | MethodDisposition::RegistersTooSmall => summary.other_skipped += 1,
        }
    }
    summary
}

impl DexResearchSummary {
    pub(super) fn merge(&mut self, other: &Self) {
        self.methods_total += other.methods_total;
        self.methods_with_code += other.methods_with_code;
        self.eligible_methods += other.eligible_methods;
        self.instruction_bytes += other.instruction_bytes;
        self.eligible_instruction_bytes += other.eligible_instruction_bytes;
        self.constructors += other.constructors;
        self.static_initializers += other.static_initializers;
        self.native_methods += other.native_methods;
        self.abstract_methods += other.abstract_methods;
        self.other_skipped += other.other_skipped;
    }
}

impl fmt::Display for DexResearchSummary {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}\t{}\t{}\t{:.2}%\t{}\t{}\t{:.2}%\t{}\t{}\t{}\t{}\t{}",
            self.methods_total,
            self.methods_with_code,
            self.eligible_methods,
            percentage(self.eligible_methods as u64, self.methods_total as u64),
            self.instruction_bytes,
            self.eligible_instruction_bytes,
            percentage(self.eligible_instruction_bytes, self.instruction_bytes),
            self.constructors,
            self.static_initializers,
            self.native_methods,
            self.abstract_methods,
            self.other_skipped,
        )
    }
}

fn percentage(value: u64, total: u64) -> f64 {
    if total == 0 {
        0.0
    } else {
        value as f64 * 100.0 / total as f64
    }
}

#[cfg(test)]
mod tests {
    use super::{percentage, summarize, DexResearchSummary};
    use crate::dex_research::{CodeInventory, DexInventory, MethodDisposition, MethodInventory};

    #[test]
    fn 汇总候选方法与指令字节() {
        let inventory = DexInventory {
            version: "035".to_owned(),
            methods: vec![
                method(MethodDisposition::Eligible, Some(6)),
                method(MethodDisposition::Constructor, Some(2)),
                method(MethodDisposition::Native, None),
                method(MethodDisposition::Abstract, None),
                method(MethodDisposition::RegistersTooSmall, Some(3)),
            ],
        };

        let summary = summarize(&inventory);
        assert_eq!(summary.methods_total, 5);
        assert_eq!(summary.methods_with_code, 3);
        assert_eq!(summary.eligible_methods, 1);
        assert_eq!(summary.instruction_bytes, 22);
        assert_eq!(summary.eligible_instruction_bytes, 12);
        assert_eq!(summary.constructors, 1);
        assert_eq!(summary.native_methods, 1);
        assert_eq!(summary.abstract_methods, 1);
        assert_eq!(summary.other_skipped, 1);
    }

    #[test]
    fn 空集合百分比为零() {
        assert_eq!(percentage(0, 0), 0.0);
    }

    #[test]
    fn 合并多_dex_汇总() {
        let mut total = DexResearchSummary::default();
        let first = summarize(&DexInventory {
            version: "035".to_owned(),
            methods: vec![method(MethodDisposition::Eligible, Some(6))],
        });
        let second = summarize(&DexInventory {
            version: "035".to_owned(),
            methods: vec![method(MethodDisposition::Constructor, Some(2))],
        });

        total.merge(&first);
        total.merge(&second);

        assert_eq!(total.methods_total, 2);
        assert_eq!(total.methods_with_code, 2);
        assert_eq!(total.eligible_methods, 1);
        assert_eq!(total.instruction_bytes, 16);
        assert_eq!(total.eligible_instruction_bytes, 12);
        assert_eq!(total.constructors, 1);
    }

    fn method(disposition: MethodDisposition, instructions_size: Option<u32>) -> MethodInventory {
        MethodInventory {
            method_index: 0,
            class_descriptor: "Lexample/Test;".to_owned(),
            name: "value".to_owned(),
            prototype: "()I".to_owned(),
            access_flags: 0,
            code_offset: instructions_size.map(|_| 1),
            code: instructions_size.map(|instructions_size| CodeInventory {
                instructions_offset: 1,
                registers_size: 1,
                tries_size: 0,
                debug_info_offset: None,
                instructions_size,
            }),
            disposition,
        }
    }
}
