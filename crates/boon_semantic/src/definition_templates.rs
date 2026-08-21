use boon_checked::{
    CheckedCallId, CheckedDefinitionExecutionNodeV1, CheckedDefinitionExecutionTemplateV1,
    CheckedExprId, CheckedListId, CheckedSourceId, CheckedStateId, DeclId,
};

/// One borrowed definition-template surface regardless of whether compilation
/// entered through the compact kernel authority or the explicit rich oracle.
///
/// The production variant never constructs a `CheckedDefinitionExecution*`
/// DTO. Keeping the distinction behind borrowed methods lets consumers move
/// to the packed authority without making `Cow` or another owned adapter the
/// new phase boundary.
#[derive(Clone, Copy)]
pub(crate) enum DefinitionExecutionTemplateRef<'a> {
    Rich(&'a CheckedDefinitionExecutionTemplateV1),
    Kernel(boon_compiler_kernel::KernelSemanticDefinitionExecutionTemplateRef<'a>),
}

#[derive(Clone, Copy)]
pub(crate) enum DefinitionExecutionNodeRef<'a> {
    Rich(&'a CheckedDefinitionExecutionNodeV1),
    Kernel(boon_compiler_kernel::KernelSemanticDefinitionExecutionNodeRef<'a>),
}

#[derive(Clone, Copy)]
pub(crate) enum DefinitionSelectorRef<'a> {
    Rich(&'a boon_checked::CheckedDefinitionSelectorV1),
    Kernel(boon_compiler_kernel::KernelSemanticDefinitionSelectorRef<'a>),
}

pub(crate) enum DefinitionExecutionNodeIter<'a> {
    Rich(std::slice::Iter<'a, CheckedDefinitionExecutionNodeV1>),
    Kernel(boon_compiler_kernel::KernelSemanticDefinitionExecutionNodeIter<'a>),
}

impl<'a> Iterator for DefinitionExecutionNodeIter<'a> {
    type Item = DefinitionExecutionNodeRef<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Rich(nodes) => nodes.next().map(DefinitionExecutionNodeRef::Rich),
            Self::Kernel(nodes) => nodes.next().map(DefinitionExecutionNodeRef::Kernel),
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        match self {
            Self::Rich(nodes) => nodes.size_hint(),
            Self::Kernel(nodes) => nodes.size_hint(),
        }
    }
}

impl ExactSizeIterator for DefinitionExecutionNodeIter<'_> {}

pub(crate) enum DefinitionExecutionTemplateIter<'a> {
    Rich(std::slice::Iter<'a, CheckedDefinitionExecutionTemplateV1>),
    Kernel(boon_compiler_kernel::KernelSemanticDefinitionExecutionTemplateIter<'a>),
}

impl<'a> Iterator for DefinitionExecutionTemplateIter<'a> {
    type Item = DefinitionExecutionTemplateRef<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Rich(templates) => templates.next().map(DefinitionExecutionTemplateRef::Rich),
            Self::Kernel(templates) => templates.next().map(DefinitionExecutionTemplateRef::Kernel),
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        match self {
            Self::Rich(templates) => templates.size_hint(),
            Self::Kernel(templates) => templates.size_hint(),
        }
    }
}

impl ExactSizeIterator for DefinitionExecutionTemplateIter<'_> {}

macro_rules! definition_value_iter {
    ($name:ident, $item:ty, $kernel:ty) => {
        pub(crate) enum $name<'a> {
            Rich(std::iter::Copied<std::slice::Iter<'a, $item>>),
            Kernel($kernel),
        }

        impl<'a> Iterator for $name<'a> {
            type Item = $item;

            fn next(&mut self) -> Option<Self::Item> {
                match self {
                    Self::Rich(rows) => rows.next(),
                    Self::Kernel(rows) => rows.next(),
                }
            }

            fn size_hint(&self) -> (usize, Option<usize>) {
                match self {
                    Self::Rich(rows) => rows.size_hint(),
                    Self::Kernel(rows) => rows.size_hint(),
                }
            }
        }

        impl ExactSizeIterator for $name<'_> {}
    };
}

definition_value_iter!(
    DefinitionExecutionExpressionIter,
    CheckedExprId,
    boon_compiler_kernel::KernelSemanticDefinitionExpressionIter<'a>
);
definition_value_iter!(
    DefinitionExecutionCallIter,
    CheckedCallId,
    boon_compiler_kernel::KernelSemanticDefinitionCallIter<'a>
);
definition_value_iter!(
    DefinitionExecutionSourceIter,
    CheckedSourceId,
    boon_compiler_kernel::KernelSemanticDefinitionSourceIter<'a>
);
definition_value_iter!(
    DefinitionExecutionStateIter,
    CheckedStateId,
    boon_compiler_kernel::KernelSemanticDefinitionStateIter<'a>
);
definition_value_iter!(
    DefinitionExecutionListIter,
    CheckedListId,
    boon_compiler_kernel::KernelSemanticDefinitionListIter<'a>
);

pub(crate) fn definition_execution_templates<'a>(
    program: &'a boon_checked::CheckedProgramFields,
    kernel_input: Option<&'a boon_compiler_kernel::KernelSemanticInputV1>,
) -> DefinitionExecutionTemplateIter<'a> {
    match kernel_input {
        Some(input) => {
            DefinitionExecutionTemplateIter::Kernel(input.definition_execution_templates())
        }
        None => {
            DefinitionExecutionTemplateIter::Rich(program.definition_execution_templates.iter())
        }
    }
}

pub(crate) fn definition_execution_template<'a>(
    program: &'a boon_checked::CheckedProgramFields,
    kernel_input: Option<&'a boon_compiler_kernel::KernelSemanticInputV1>,
    callable: DeclId,
) -> Option<DefinitionExecutionTemplateRef<'a>> {
    match kernel_input {
        Some(input) => input
            .definition_execution_template(callable)
            .map(DefinitionExecutionTemplateRef::Kernel),
        None => program
            .definition_execution_templates
            .iter()
            .find(|template| template.callable == callable)
            .map(DefinitionExecutionTemplateRef::Rich),
    }
}

impl<'a> DefinitionExecutionTemplateRef<'a> {
    pub(crate) fn has_expected_schema(self) -> bool {
        match self {
            Self::Rich(template) => {
                template.schema == boon_checked::CHECKED_DEFINITION_EXECUTION_TEMPLATE_SCHEMA_V1
            }
            Self::Kernel(_) => true,
        }
    }

    pub(crate) fn callable(self) -> DeclId {
        match self {
            Self::Rich(template) => template.callable,
            Self::Kernel(template) => template.callable(),
        }
    }

    pub(crate) fn result(self) -> CheckedExprId {
        match self {
            Self::Rich(template) => template.result,
            Self::Kernel(template) => template.result(),
        }
    }

    pub(crate) fn nodes(self) -> DefinitionExecutionNodeIter<'a> {
        match self {
            Self::Rich(template) => DefinitionExecutionNodeIter::Rich(template.nodes.iter()),
            Self::Kernel(template) => DefinitionExecutionNodeIter::Kernel(template.nodes()),
        }
    }

    pub(crate) fn node_count(self) -> usize {
        match self {
            Self::Rich(template) => template.nodes.len(),
            Self::Kernel(template) => template.nodes().len(),
        }
    }

    pub(crate) fn calls(self) -> DefinitionExecutionCallIter<'a> {
        match self {
            Self::Rich(template) => {
                DefinitionExecutionCallIter::Rich(template.calls.iter().copied())
            }
            Self::Kernel(template) => DefinitionExecutionCallIter::Kernel(template.calls()),
        }
    }

    pub(crate) fn sources(self) -> DefinitionExecutionSourceIter<'a> {
        match self {
            Self::Rich(template) => {
                DefinitionExecutionSourceIter::Rich(template.sources.iter().copied())
            }
            Self::Kernel(template) => DefinitionExecutionSourceIter::Kernel(template.sources()),
        }
    }

    pub(crate) fn states(self) -> DefinitionExecutionStateIter<'a> {
        match self {
            Self::Rich(template) => {
                DefinitionExecutionStateIter::Rich(template.states.iter().copied())
            }
            Self::Kernel(template) => DefinitionExecutionStateIter::Kernel(template.states()),
        }
    }

    pub(crate) fn lists(self) -> DefinitionExecutionListIter<'a> {
        match self {
            Self::Rich(template) => {
                DefinitionExecutionListIter::Rich(template.lists.iter().copied())
            }
            Self::Kernel(template) => DefinitionExecutionListIter::Kernel(template.lists()),
        }
    }
}

impl<'a> DefinitionExecutionNodeRef<'a> {
    pub(crate) fn expression(self) -> CheckedExprId {
        match self {
            Self::Rich(node) => node.expression,
            Self::Kernel(node) => node.expression(),
        }
    }

    pub(crate) fn dependencies(self) -> DefinitionExecutionExpressionIter<'a> {
        match self {
            Self::Rich(node) => {
                DefinitionExecutionExpressionIter::Rich(node.dependencies.iter().copied())
            }
            Self::Kernel(node) => DefinitionExecutionExpressionIter::Kernel(node.dependencies()),
        }
    }

    pub(crate) fn call(self) -> Option<CheckedCallId> {
        match self {
            Self::Rich(node) => node.call,
            Self::Kernel(node) => node.call(),
        }
    }

    pub(crate) fn selector(self) -> Option<DefinitionSelectorRef<'a>> {
        match self {
            Self::Rich(node) => node.selector.as_ref().map(DefinitionSelectorRef::Rich),
            Self::Kernel(node) => node.selector().map(DefinitionSelectorRef::Kernel),
        }
    }
}

impl<'a> DefinitionSelectorRef<'a> {
    pub(crate) fn input(self) -> CheckedExprId {
        match self {
            Self::Rich(selector) => selector.input,
            Self::Kernel(selector) => selector.input(),
        }
    }

    pub(crate) fn arms(self) -> DefinitionExecutionExpressionIter<'a> {
        match self {
            Self::Rich(selector) => {
                DefinitionExecutionExpressionIter::Rich(selector.arms.iter().copied())
            }
            Self::Kernel(selector) => DefinitionExecutionExpressionIter::Kernel(selector.arms()),
        }
    }
}
