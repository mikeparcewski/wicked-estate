#include "tree_sitter/parser.h"

#if defined(__GNUC__) || defined(__clang__)
#pragma GCC diagnostic ignored "-Wmissing-field-initializers"
#endif

#ifdef _MSC_VER
#pragma optimize("", off)
#elif defined(__clang__)
#pragma clang optimize off
#elif defined(__GNUC__)
#pragma GCC optimize ("O0")
#endif

#define LANGUAGE_VERSION 14
#define STATE_COUNT 335
#define LARGE_STATE_COUNT 2
#define SYMBOL_COUNT 130
#define ALIAS_COUNT 0
#define TOKEN_COUNT 82
#define EXTERNAL_TOKEN_COUNT 0
#define FIELD_COUNT 0
#define MAX_ALIAS_SEQUENCE_LENGTH 13
#define PRODUCTION_ID_COUNT 1

enum ts_symbol_identifiers {
  sym_comment = 1,
  anon_sym_Exit = 2,
  anon_sym_For = 3,
  anon_sym_Function = 4,
  anon_sym_Sub = 5,
  anon_sym_Do = 6,
  anon_sym_While = 7,
  anon_sym_If = 8,
  anon_sym_Then = 9,
  anon_sym_Else = 10,
  anon_sym_EndIf = 11,
  anon_sym_To = 12,
  anon_sym_Step = 13,
  anon_sym_Next = 14,
  anon_sym_Wend = 15,
  anon_sym_Loop = 16,
  anon_sym_Until = 17,
  anon_sym_LPAREN = 18,
  anon_sym_RPAREN = 19,
  anon_sym_EndSub = 20,
  anon_sym_Private = 21,
  anon_sym_EndFunction = 22,
  anon_sym_Dim = 23,
  anon_sym_ReDim = 24,
  anon_sym_Preserve = 25,
  anon_sym_As = 26,
  anon_sym_COMMA = 27,
  anon_sym_Declare = 28,
  anon_sym_PtrSafe = 29,
  anon_sym_Lib = 30,
  anon_sym_Alias = 31,
  anon_sym_ByVal = 32,
  anon_sym_ByRef = 33,
  anon_sym_Optional = 34,
  anon_sym_ParamArray = 35,
  anon_sym_Any = 36,
  anon_sym_Boolean = 37,
  anon_sym_Byte = 38,
  anon_sym_Collection = 39,
  anon_sym_Currency = 40,
  anon_sym_Date = 41,
  anon_sym_Decimal = 42,
  anon_sym_Dictionary = 43,
  anon_sym_Double = 44,
  anon_sym_Integer = 45,
  anon_sym_Long = 46,
  anon_sym_LongLong = 47,
  anon_sym_LongPtr = 48,
  anon_sym_Object = 49,
  anon_sym_Single = 50,
  anon_sym_String = 51,
  anon_sym_Variant = 52,
  anon_sym_LPAREN_RPAREN = 53,
  anon_sym_New = 54,
  anon_sym_DOT = 55,
  anon_sym_PLUS = 56,
  anon_sym_DASH = 57,
  anon_sym_STAR = 58,
  anon_sym_SLASH = 59,
  anon_sym_Mod = 60,
  anon_sym_AMP = 61,
  aux_sym_binary_expression_token1 = 62,
  aux_sym_binary_expression_token2 = 63,
  aux_sym_binary_expression_token3 = 64,
  anon_sym_LT_GT = 65,
  anon_sym_LT = 66,
  anon_sym_GT = 67,
  anon_sym_LT_EQ = 68,
  anon_sym_GT_EQ = 69,
  anon_sym_Not = 70,
  anon_sym_Call = 71,
  anon_sym_COLON_EQ = 72,
  sym_number = 73,
  anon_sym_DQUOTE = 74,
  aux_sym_string_literal_token1 = 75,
  anon_sym_True = 76,
  anon_sym_False = 77,
  sym_identifier = 78,
  sym__equal = 79,
  aux_sym__whitespace_token1 = 80,
  sym__horizontal_whitespace = 81,
  sym_source_file = 82,
  sym__statement = 83,
  sym__inline_statement = 84,
  sym_variable_assignment = 85,
  sym__inline_statement_block = 86,
  sym_invocation_statement = 87,
  sym_exit_statement = 88,
  sym_if_statement = 89,
  sym_for_statement = 90,
  sym_while_statement = 91,
  sym_do_statement = 92,
  sym_subroutine = 93,
  sym_function = 94,
  sym_variable_declaration = 95,
  sym_redim = 96,
  sym__variable_declaration_assignment = 97,
  sym_variable_declaration_identifier = 98,
  sym_array_identifier = 99,
  sym_type_definition = 100,
  sym_variable_list = 101,
  sym_ptrsafe_function_declaration = 102,
  sym_parameter_list = 103,
  sym_parameter = 104,
  sym_modifier = 105,
  sym_type = 106,
  sym_type_terminal = 107,
  sym_array_type = 108,
  sym__expression = 109,
  sym_new_expression = 110,
  sym_type_member_expression = 111,
  sym_member_expression = 112,
  sym_binary_expression = 113,
  sym_unary_expression = 114,
  sym_function_call = 115,
  sym_argument_list = 116,
  sym_argument = 117,
  sym_keyword_argument = 118,
  sym_array_element = 119,
  sym_literal = 120,
  sym_string_literal = 121,
  sym_boolean = 122,
  sym_new_identifier = 123,
  aux_sym__whitespace = 124,
  aux_sym_source_file_repeat1 = 125,
  aux_sym__inline_statement_block_repeat1 = 126,
  aux_sym_variable_list_repeat1 = 127,
  aux_sym_parameter_list_repeat1 = 128,
  aux_sym_argument_list_repeat1 = 129,
};

static const char * const ts_symbol_names[] = {
  [ts_builtin_sym_end] = "end",
  [sym_comment] = "comment",
  [anon_sym_Exit] = "Exit",
  [anon_sym_For] = "For",
  [anon_sym_Function] = "Function",
  [anon_sym_Sub] = "Sub",
  [anon_sym_Do] = "Do",
  [anon_sym_While] = "While",
  [anon_sym_If] = "If",
  [anon_sym_Then] = "Then",
  [anon_sym_Else] = "Else",
  [anon_sym_EndIf] = "End If",
  [anon_sym_To] = "To",
  [anon_sym_Step] = "Step",
  [anon_sym_Next] = "Next",
  [anon_sym_Wend] = "Wend",
  [anon_sym_Loop] = "Loop",
  [anon_sym_Until] = "Until",
  [anon_sym_LPAREN] = "(",
  [anon_sym_RPAREN] = ")",
  [anon_sym_EndSub] = "End Sub",
  [anon_sym_Private] = "Private",
  [anon_sym_EndFunction] = "End Function",
  [anon_sym_Dim] = "Dim",
  [anon_sym_ReDim] = "ReDim",
  [anon_sym_Preserve] = "Preserve",
  [anon_sym_As] = "As",
  [anon_sym_COMMA] = ",",
  [anon_sym_Declare] = "Declare",
  [anon_sym_PtrSafe] = "PtrSafe",
  [anon_sym_Lib] = "Lib",
  [anon_sym_Alias] = "Alias",
  [anon_sym_ByVal] = "ByVal",
  [anon_sym_ByRef] = "ByRef",
  [anon_sym_Optional] = "Optional",
  [anon_sym_ParamArray] = "ParamArray",
  [anon_sym_Any] = "Any",
  [anon_sym_Boolean] = "Boolean",
  [anon_sym_Byte] = "Byte",
  [anon_sym_Collection] = "Collection",
  [anon_sym_Currency] = "Currency",
  [anon_sym_Date] = "Date",
  [anon_sym_Decimal] = "Decimal",
  [anon_sym_Dictionary] = "Dictionary",
  [anon_sym_Double] = "Double",
  [anon_sym_Integer] = "Integer",
  [anon_sym_Long] = "Long",
  [anon_sym_LongLong] = "LongLong",
  [anon_sym_LongPtr] = "LongPtr",
  [anon_sym_Object] = "Object",
  [anon_sym_Single] = "Single",
  [anon_sym_String] = "String",
  [anon_sym_Variant] = "Variant",
  [anon_sym_LPAREN_RPAREN] = "()",
  [anon_sym_New] = "New",
  [anon_sym_DOT] = ".",
  [anon_sym_PLUS] = "+",
  [anon_sym_DASH] = "-",
  [anon_sym_STAR] = "*",
  [anon_sym_SLASH] = "/",
  [anon_sym_Mod] = "Mod",
  [anon_sym_AMP] = "&",
  [aux_sym_binary_expression_token1] = "binary_expression_token1",
  [aux_sym_binary_expression_token2] = "binary_expression_token2",
  [aux_sym_binary_expression_token3] = "binary_expression_token3",
  [anon_sym_LT_GT] = "<>",
  [anon_sym_LT] = "<",
  [anon_sym_GT] = ">",
  [anon_sym_LT_EQ] = "<=",
  [anon_sym_GT_EQ] = ">=",
  [anon_sym_Not] = "Not",
  [anon_sym_Call] = "Call",
  [anon_sym_COLON_EQ] = ":=",
  [sym_number] = "number",
  [anon_sym_DQUOTE] = "\"",
  [aux_sym_string_literal_token1] = "string_literal_token1",
  [anon_sym_True] = "True",
  [anon_sym_False] = "False",
  [sym_identifier] = "identifier",
  [sym__equal] = "_equal",
  [aux_sym__whitespace_token1] = "_whitespace_token1",
  [sym__horizontal_whitespace] = "_horizontal_whitespace",
  [sym_source_file] = "source_file",
  [sym__statement] = "_statement",
  [sym__inline_statement] = "_inline_statement",
  [sym_variable_assignment] = "variable_assignment",
  [sym__inline_statement_block] = "_inline_statement_block",
  [sym_invocation_statement] = "invocation_statement",
  [sym_exit_statement] = "exit_statement",
  [sym_if_statement] = "if_statement",
  [sym_for_statement] = "for_statement",
  [sym_while_statement] = "while_statement",
  [sym_do_statement] = "do_statement",
  [sym_subroutine] = "subroutine",
  [sym_function] = "function",
  [sym_variable_declaration] = "variable_declaration",
  [sym_redim] = "redim",
  [sym__variable_declaration_assignment] = "_variable_declaration_assignment",
  [sym_variable_declaration_identifier] = "variable_declaration_identifier",
  [sym_array_identifier] = "array_identifier",
  [sym_type_definition] = "type_definition",
  [sym_variable_list] = "variable_list",
  [sym_ptrsafe_function_declaration] = "ptrsafe_function_declaration",
  [sym_parameter_list] = "parameter_list",
  [sym_parameter] = "parameter",
  [sym_modifier] = "modifier",
  [sym_type] = "type",
  [sym_type_terminal] = "type_terminal",
  [sym_array_type] = "array_type",
  [sym__expression] = "_expression",
  [sym_new_expression] = "new_expression",
  [sym_type_member_expression] = "type_member_expression",
  [sym_member_expression] = "member_expression",
  [sym_binary_expression] = "binary_expression",
  [sym_unary_expression] = "unary_expression",
  [sym_function_call] = "function_call",
  [sym_argument_list] = "argument_list",
  [sym_argument] = "argument",
  [sym_keyword_argument] = "keyword_argument",
  [sym_array_element] = "array_element",
  [sym_literal] = "literal",
  [sym_string_literal] = "string_literal",
  [sym_boolean] = "boolean",
  [sym_new_identifier] = "new_identifier",
  [aux_sym__whitespace] = "_whitespace",
  [aux_sym_source_file_repeat1] = "source_file_repeat1",
  [aux_sym__inline_statement_block_repeat1] = "_inline_statement_block_repeat1",
  [aux_sym_variable_list_repeat1] = "variable_list_repeat1",
  [aux_sym_parameter_list_repeat1] = "parameter_list_repeat1",
  [aux_sym_argument_list_repeat1] = "argument_list_repeat1",
};

static const TSSymbol ts_symbol_map[] = {
  [ts_builtin_sym_end] = ts_builtin_sym_end,
  [sym_comment] = sym_comment,
  [anon_sym_Exit] = anon_sym_Exit,
  [anon_sym_For] = anon_sym_For,
  [anon_sym_Function] = anon_sym_Function,
  [anon_sym_Sub] = anon_sym_Sub,
  [anon_sym_Do] = anon_sym_Do,
  [anon_sym_While] = anon_sym_While,
  [anon_sym_If] = anon_sym_If,
  [anon_sym_Then] = anon_sym_Then,
  [anon_sym_Else] = anon_sym_Else,
  [anon_sym_EndIf] = anon_sym_EndIf,
  [anon_sym_To] = anon_sym_To,
  [anon_sym_Step] = anon_sym_Step,
  [anon_sym_Next] = anon_sym_Next,
  [anon_sym_Wend] = anon_sym_Wend,
  [anon_sym_Loop] = anon_sym_Loop,
  [anon_sym_Until] = anon_sym_Until,
  [anon_sym_LPAREN] = anon_sym_LPAREN,
  [anon_sym_RPAREN] = anon_sym_RPAREN,
  [anon_sym_EndSub] = anon_sym_EndSub,
  [anon_sym_Private] = anon_sym_Private,
  [anon_sym_EndFunction] = anon_sym_EndFunction,
  [anon_sym_Dim] = anon_sym_Dim,
  [anon_sym_ReDim] = anon_sym_ReDim,
  [anon_sym_Preserve] = anon_sym_Preserve,
  [anon_sym_As] = anon_sym_As,
  [anon_sym_COMMA] = anon_sym_COMMA,
  [anon_sym_Declare] = anon_sym_Declare,
  [anon_sym_PtrSafe] = anon_sym_PtrSafe,
  [anon_sym_Lib] = anon_sym_Lib,
  [anon_sym_Alias] = anon_sym_Alias,
  [anon_sym_ByVal] = anon_sym_ByVal,
  [anon_sym_ByRef] = anon_sym_ByRef,
  [anon_sym_Optional] = anon_sym_Optional,
  [anon_sym_ParamArray] = anon_sym_ParamArray,
  [anon_sym_Any] = anon_sym_Any,
  [anon_sym_Boolean] = anon_sym_Boolean,
  [anon_sym_Byte] = anon_sym_Byte,
  [anon_sym_Collection] = anon_sym_Collection,
  [anon_sym_Currency] = anon_sym_Currency,
  [anon_sym_Date] = anon_sym_Date,
  [anon_sym_Decimal] = anon_sym_Decimal,
  [anon_sym_Dictionary] = anon_sym_Dictionary,
  [anon_sym_Double] = anon_sym_Double,
  [anon_sym_Integer] = anon_sym_Integer,
  [anon_sym_Long] = anon_sym_Long,
  [anon_sym_LongLong] = anon_sym_LongLong,
  [anon_sym_LongPtr] = anon_sym_LongPtr,
  [anon_sym_Object] = anon_sym_Object,
  [anon_sym_Single] = anon_sym_Single,
  [anon_sym_String] = anon_sym_String,
  [anon_sym_Variant] = anon_sym_Variant,
  [anon_sym_LPAREN_RPAREN] = anon_sym_LPAREN_RPAREN,
  [anon_sym_New] = anon_sym_New,
  [anon_sym_DOT] = anon_sym_DOT,
  [anon_sym_PLUS] = anon_sym_PLUS,
  [anon_sym_DASH] = anon_sym_DASH,
  [anon_sym_STAR] = anon_sym_STAR,
  [anon_sym_SLASH] = anon_sym_SLASH,
  [anon_sym_Mod] = anon_sym_Mod,
  [anon_sym_AMP] = anon_sym_AMP,
  [aux_sym_binary_expression_token1] = aux_sym_binary_expression_token1,
  [aux_sym_binary_expression_token2] = aux_sym_binary_expression_token2,
  [aux_sym_binary_expression_token3] = aux_sym_binary_expression_token3,
  [anon_sym_LT_GT] = anon_sym_LT_GT,
  [anon_sym_LT] = anon_sym_LT,
  [anon_sym_GT] = anon_sym_GT,
  [anon_sym_LT_EQ] = anon_sym_LT_EQ,
  [anon_sym_GT_EQ] = anon_sym_GT_EQ,
  [anon_sym_Not] = anon_sym_Not,
  [anon_sym_Call] = anon_sym_Call,
  [anon_sym_COLON_EQ] = anon_sym_COLON_EQ,
  [sym_number] = sym_number,
  [anon_sym_DQUOTE] = anon_sym_DQUOTE,
  [aux_sym_string_literal_token1] = aux_sym_string_literal_token1,
  [anon_sym_True] = anon_sym_True,
  [anon_sym_False] = anon_sym_False,
  [sym_identifier] = sym_identifier,
  [sym__equal] = sym__equal,
  [aux_sym__whitespace_token1] = aux_sym__whitespace_token1,
  [sym__horizontal_whitespace] = sym__horizontal_whitespace,
  [sym_source_file] = sym_source_file,
  [sym__statement] = sym__statement,
  [sym__inline_statement] = sym__inline_statement,
  [sym_variable_assignment] = sym_variable_assignment,
  [sym__inline_statement_block] = sym__inline_statement_block,
  [sym_invocation_statement] = sym_invocation_statement,
  [sym_exit_statement] = sym_exit_statement,
  [sym_if_statement] = sym_if_statement,
  [sym_for_statement] = sym_for_statement,
  [sym_while_statement] = sym_while_statement,
  [sym_do_statement] = sym_do_statement,
  [sym_subroutine] = sym_subroutine,
  [sym_function] = sym_function,
  [sym_variable_declaration] = sym_variable_declaration,
  [sym_redim] = sym_redim,
  [sym__variable_declaration_assignment] = sym__variable_declaration_assignment,
  [sym_variable_declaration_identifier] = sym_variable_declaration_identifier,
  [sym_array_identifier] = sym_array_identifier,
  [sym_type_definition] = sym_type_definition,
  [sym_variable_list] = sym_variable_list,
  [sym_ptrsafe_function_declaration] = sym_ptrsafe_function_declaration,
  [sym_parameter_list] = sym_parameter_list,
  [sym_parameter] = sym_parameter,
  [sym_modifier] = sym_modifier,
  [sym_type] = sym_type,
  [sym_type_terminal] = sym_type_terminal,
  [sym_array_type] = sym_array_type,
  [sym__expression] = sym__expression,
  [sym_new_expression] = sym_new_expression,
  [sym_type_member_expression] = sym_type_member_expression,
  [sym_member_expression] = sym_member_expression,
  [sym_binary_expression] = sym_binary_expression,
  [sym_unary_expression] = sym_unary_expression,
  [sym_function_call] = sym_function_call,
  [sym_argument_list] = sym_argument_list,
  [sym_argument] = sym_argument,
  [sym_keyword_argument] = sym_keyword_argument,
  [sym_array_element] = sym_array_element,
  [sym_literal] = sym_literal,
  [sym_string_literal] = sym_string_literal,
  [sym_boolean] = sym_boolean,
  [sym_new_identifier] = sym_new_identifier,
  [aux_sym__whitespace] = aux_sym__whitespace,
  [aux_sym_source_file_repeat1] = aux_sym_source_file_repeat1,
  [aux_sym__inline_statement_block_repeat1] = aux_sym__inline_statement_block_repeat1,
  [aux_sym_variable_list_repeat1] = aux_sym_variable_list_repeat1,
  [aux_sym_parameter_list_repeat1] = aux_sym_parameter_list_repeat1,
  [aux_sym_argument_list_repeat1] = aux_sym_argument_list_repeat1,
};

static const TSSymbolMetadata ts_symbol_metadata[] = {
  [ts_builtin_sym_end] = {
    .visible = false,
    .named = true,
  },
  [sym_comment] = {
    .visible = true,
    .named = true,
  },
  [anon_sym_Exit] = {
    .visible = true,
    .named = false,
  },
  [anon_sym_For] = {
    .visible = true,
    .named = false,
  },
  [anon_sym_Function] = {
    .visible = true,
    .named = false,
  },
  [anon_sym_Sub] = {
    .visible = true,
    .named = false,
  },
  [anon_sym_Do] = {
    .visible = true,
    .named = false,
  },
  [anon_sym_While] = {
    .visible = true,
    .named = false,
  },
  [anon_sym_If] = {
    .visible = true,
    .named = false,
  },
  [anon_sym_Then] = {
    .visible = true,
    .named = false,
  },
  [anon_sym_Else] = {
    .visible = true,
    .named = false,
  },
  [anon_sym_EndIf] = {
    .visible = true,
    .named = false,
  },
  [anon_sym_To] = {
    .visible = true,
    .named = false,
  },
  [anon_sym_Step] = {
    .visible = true,
    .named = false,
  },
  [anon_sym_Next] = {
    .visible = true,
    .named = false,
  },
  [anon_sym_Wend] = {
    .visible = true,
    .named = false,
  },
  [anon_sym_Loop] = {
    .visible = true,
    .named = false,
  },
  [anon_sym_Until] = {
    .visible = true,
    .named = false,
  },
  [anon_sym_LPAREN] = {
    .visible = true,
    .named = false,
  },
  [anon_sym_RPAREN] = {
    .visible = true,
    .named = false,
  },
  [anon_sym_EndSub] = {
    .visible = true,
    .named = false,
  },
  [anon_sym_Private] = {
    .visible = true,
    .named = false,
  },
  [anon_sym_EndFunction] = {
    .visible = true,
    .named = false,
  },
  [anon_sym_Dim] = {
    .visible = true,
    .named = false,
  },
  [anon_sym_ReDim] = {
    .visible = true,
    .named = false,
  },
  [anon_sym_Preserve] = {
    .visible = true,
    .named = false,
  },
  [anon_sym_As] = {
    .visible = true,
    .named = false,
  },
  [anon_sym_COMMA] = {
    .visible = true,
    .named = false,
  },
  [anon_sym_Declare] = {
    .visible = true,
    .named = false,
  },
  [anon_sym_PtrSafe] = {
    .visible = true,
    .named = false,
  },
  [anon_sym_Lib] = {
    .visible = true,
    .named = false,
  },
  [anon_sym_Alias] = {
    .visible = true,
    .named = false,
  },
  [anon_sym_ByVal] = {
    .visible = true,
    .named = false,
  },
  [anon_sym_ByRef] = {
    .visible = true,
    .named = false,
  },
  [anon_sym_Optional] = {
    .visible = true,
    .named = false,
  },
  [anon_sym_ParamArray] = {
    .visible = true,
    .named = false,
  },
  [anon_sym_Any] = {
    .visible = true,
    .named = false,
  },
  [anon_sym_Boolean] = {
    .visible = true,
    .named = false,
  },
  [anon_sym_Byte] = {
    .visible = true,
    .named = false,
  },
  [anon_sym_Collection] = {
    .visible = true,
    .named = false,
  },
  [anon_sym_Currency] = {
    .visible = true,
    .named = false,
  },
  [anon_sym_Date] = {
    .visible = true,
    .named = false,
  },
  [anon_sym_Decimal] = {
    .visible = true,
    .named = false,
  },
  [anon_sym_Dictionary] = {
    .visible = true,
    .named = false,
  },
  [anon_sym_Double] = {
    .visible = true,
    .named = false,
  },
  [anon_sym_Integer] = {
    .visible = true,
    .named = false,
  },
  [anon_sym_Long] = {
    .visible = true,
    .named = false,
  },
  [anon_sym_LongLong] = {
    .visible = true,
    .named = false,
  },
  [anon_sym_LongPtr] = {
    .visible = true,
    .named = false,
  },
  [anon_sym_Object] = {
    .visible = true,
    .named = false,
  },
  [anon_sym_Single] = {
    .visible = true,
    .named = false,
  },
  [anon_sym_String] = {
    .visible = true,
    .named = false,
  },
  [anon_sym_Variant] = {
    .visible = true,
    .named = false,
  },
  [anon_sym_LPAREN_RPAREN] = {
    .visible = true,
    .named = false,
  },
  [anon_sym_New] = {
    .visible = true,
    .named = false,
  },
  [anon_sym_DOT] = {
    .visible = true,
    .named = false,
  },
  [anon_sym_PLUS] = {
    .visible = true,
    .named = false,
  },
  [anon_sym_DASH] = {
    .visible = true,
    .named = false,
  },
  [anon_sym_STAR] = {
    .visible = true,
    .named = false,
  },
  [anon_sym_SLASH] = {
    .visible = true,
    .named = false,
  },
  [anon_sym_Mod] = {
    .visible = true,
    .named = false,
  },
  [anon_sym_AMP] = {
    .visible = true,
    .named = false,
  },
  [aux_sym_binary_expression_token1] = {
    .visible = false,
    .named = false,
  },
  [aux_sym_binary_expression_token2] = {
    .visible = false,
    .named = false,
  },
  [aux_sym_binary_expression_token3] = {
    .visible = false,
    .named = false,
  },
  [anon_sym_LT_GT] = {
    .visible = true,
    .named = false,
  },
  [anon_sym_LT] = {
    .visible = true,
    .named = false,
  },
  [anon_sym_GT] = {
    .visible = true,
    .named = false,
  },
  [anon_sym_LT_EQ] = {
    .visible = true,
    .named = false,
  },
  [anon_sym_GT_EQ] = {
    .visible = true,
    .named = false,
  },
  [anon_sym_Not] = {
    .visible = true,
    .named = false,
  },
  [anon_sym_Call] = {
    .visible = true,
    .named = false,
  },
  [anon_sym_COLON_EQ] = {
    .visible = true,
    .named = false,
  },
  [sym_number] = {
    .visible = true,
    .named = true,
  },
  [anon_sym_DQUOTE] = {
    .visible = true,
    .named = false,
  },
  [aux_sym_string_literal_token1] = {
    .visible = false,
    .named = false,
  },
  [anon_sym_True] = {
    .visible = true,
    .named = false,
  },
  [anon_sym_False] = {
    .visible = true,
    .named = false,
  },
  [sym_identifier] = {
    .visible = true,
    .named = true,
  },
  [sym__equal] = {
    .visible = false,
    .named = true,
  },
  [aux_sym__whitespace_token1] = {
    .visible = false,
    .named = false,
  },
  [sym__horizontal_whitespace] = {
    .visible = false,
    .named = true,
  },
  [sym_source_file] = {
    .visible = true,
    .named = true,
  },
  [sym__statement] = {
    .visible = false,
    .named = true,
  },
  [sym__inline_statement] = {
    .visible = false,
    .named = true,
  },
  [sym_variable_assignment] = {
    .visible = true,
    .named = true,
  },
  [sym__inline_statement_block] = {
    .visible = false,
    .named = true,
  },
  [sym_invocation_statement] = {
    .visible = true,
    .named = true,
  },
  [sym_exit_statement] = {
    .visible = true,
    .named = true,
  },
  [sym_if_statement] = {
    .visible = true,
    .named = true,
  },
  [sym_for_statement] = {
    .visible = true,
    .named = true,
  },
  [sym_while_statement] = {
    .visible = true,
    .named = true,
  },
  [sym_do_statement] = {
    .visible = true,
    .named = true,
  },
  [sym_subroutine] = {
    .visible = true,
    .named = true,
  },
  [sym_function] = {
    .visible = true,
    .named = true,
  },
  [sym_variable_declaration] = {
    .visible = true,
    .named = true,
  },
  [sym_redim] = {
    .visible = true,
    .named = true,
  },
  [sym__variable_declaration_assignment] = {
    .visible = false,
    .named = true,
  },
  [sym_variable_declaration_identifier] = {
    .visible = true,
    .named = true,
  },
  [sym_array_identifier] = {
    .visible = true,
    .named = true,
  },
  [sym_type_definition] = {
    .visible = true,
    .named = true,
  },
  [sym_variable_list] = {
    .visible = true,
    .named = true,
  },
  [sym_ptrsafe_function_declaration] = {
    .visible = true,
    .named = true,
  },
  [sym_parameter_list] = {
    .visible = true,
    .named = true,
  },
  [sym_parameter] = {
    .visible = true,
    .named = true,
  },
  [sym_modifier] = {
    .visible = true,
    .named = true,
  },
  [sym_type] = {
    .visible = true,
    .named = true,
  },
  [sym_type_terminal] = {
    .visible = true,
    .named = true,
  },
  [sym_array_type] = {
    .visible = true,
    .named = true,
  },
  [sym__expression] = {
    .visible = false,
    .named = true,
  },
  [sym_new_expression] = {
    .visible = true,
    .named = true,
  },
  [sym_type_member_expression] = {
    .visible = true,
    .named = true,
  },
  [sym_member_expression] = {
    .visible = true,
    .named = true,
  },
  [sym_binary_expression] = {
    .visible = true,
    .named = true,
  },
  [sym_unary_expression] = {
    .visible = true,
    .named = true,
  },
  [sym_function_call] = {
    .visible = true,
    .named = true,
  },
  [sym_argument_list] = {
    .visible = true,
    .named = true,
  },
  [sym_argument] = {
    .visible = true,
    .named = true,
  },
  [sym_keyword_argument] = {
    .visible = true,
    .named = true,
  },
  [sym_array_element] = {
    .visible = true,
    .named = true,
  },
  [sym_literal] = {
    .visible = true,
    .named = true,
  },
  [sym_string_literal] = {
    .visible = true,
    .named = true,
  },
  [sym_boolean] = {
    .visible = true,
    .named = true,
  },
  [sym_new_identifier] = {
    .visible = true,
    .named = true,
  },
  [aux_sym__whitespace] = {
    .visible = false,
    .named = false,
  },
  [aux_sym_source_file_repeat1] = {
    .visible = false,
    .named = false,
  },
  [aux_sym__inline_statement_block_repeat1] = {
    .visible = false,
    .named = false,
  },
  [aux_sym_variable_list_repeat1] = {
    .visible = false,
    .named = false,
  },
  [aux_sym_parameter_list_repeat1] = {
    .visible = false,
    .named = false,
  },
  [aux_sym_argument_list_repeat1] = {
    .visible = false,
    .named = false,
  },
};

static const TSSymbol ts_alias_sequences[PRODUCTION_ID_COUNT][MAX_ALIAS_SEQUENCE_LENGTH] = {
  [0] = {0},
};

static const uint16_t ts_non_terminal_alias_map[] = {
  0,
};

static const TSStateId ts_primary_state_ids[STATE_COUNT] = {
  [0] = 0,
  [1] = 1,
  [2] = 2,
  [3] = 3,
  [4] = 4,
  [5] = 5,
  [6] = 6,
  [7] = 7,
  [8] = 8,
  [9] = 9,
  [10] = 10,
  [11] = 11,
  [12] = 12,
  [13] = 13,
  [14] = 14,
  [15] = 15,
  [16] = 16,
  [17] = 16,
  [18] = 18,
  [19] = 19,
  [20] = 18,
  [21] = 19,
  [22] = 18,
  [23] = 19,
  [24] = 19,
  [25] = 19,
  [26] = 18,
  [27] = 18,
  [28] = 19,
  [29] = 19,
  [30] = 18,
  [31] = 18,
  [32] = 32,
  [33] = 33,
  [34] = 34,
  [35] = 35,
  [36] = 36,
  [37] = 37,
  [38] = 38,
  [39] = 39,
  [40] = 40,
  [41] = 41,
  [42] = 42,
  [43] = 43,
  [44] = 44,
  [45] = 45,
  [46] = 46,
  [47] = 47,
  [48] = 48,
  [49] = 49,
  [50] = 50,
  [51] = 51,
  [52] = 52,
  [53] = 53,
  [54] = 54,
  [55] = 55,
  [56] = 52,
  [57] = 52,
  [58] = 58,
  [59] = 55,
  [60] = 55,
  [61] = 35,
  [62] = 48,
  [63] = 63,
  [64] = 41,
  [65] = 65,
  [66] = 58,
  [67] = 63,
  [68] = 43,
  [69] = 69,
  [70] = 42,
  [71] = 65,
  [72] = 69,
  [73] = 69,
  [74] = 58,
  [75] = 58,
  [76] = 65,
  [77] = 77,
  [78] = 38,
  [79] = 37,
  [80] = 58,
  [81] = 33,
  [82] = 49,
  [83] = 50,
  [84] = 69,
  [85] = 47,
  [86] = 45,
  [87] = 44,
  [88] = 34,
  [89] = 69,
  [90] = 41,
  [91] = 77,
  [92] = 46,
  [93] = 42,
  [94] = 77,
  [95] = 95,
  [96] = 65,
  [97] = 58,
  [98] = 69,
  [99] = 40,
  [100] = 48,
  [101] = 36,
  [102] = 58,
  [103] = 103,
  [104] = 65,
  [105] = 65,
  [106] = 58,
  [107] = 43,
  [108] = 103,
  [109] = 69,
  [110] = 65,
  [111] = 111,
  [112] = 112,
  [113] = 35,
  [114] = 49,
  [115] = 115,
  [116] = 116,
  [117] = 115,
  [118] = 116,
  [119] = 33,
  [120] = 50,
  [121] = 121,
  [122] = 116,
  [123] = 123,
  [124] = 124,
  [125] = 112,
  [126] = 126,
  [127] = 47,
  [128] = 128,
  [129] = 126,
  [130] = 128,
  [131] = 37,
  [132] = 132,
  [133] = 45,
  [134] = 121,
  [135] = 135,
  [136] = 136,
  [137] = 137,
  [138] = 34,
  [139] = 36,
  [140] = 46,
  [141] = 141,
  [142] = 142,
  [143] = 40,
  [144] = 144,
  [145] = 145,
  [146] = 146,
  [147] = 44,
  [148] = 121,
  [149] = 112,
  [150] = 115,
  [151] = 38,
  [152] = 126,
  [153] = 153,
  [154] = 154,
  [155] = 155,
  [156] = 155,
  [157] = 154,
  [158] = 158,
  [159] = 159,
  [160] = 160,
  [161] = 161,
  [162] = 162,
  [163] = 163,
  [164] = 164,
  [165] = 162,
  [166] = 166,
  [167] = 167,
  [168] = 168,
  [169] = 169,
  [170] = 163,
  [171] = 163,
  [172] = 172,
  [173] = 173,
  [174] = 174,
  [175] = 173,
  [176] = 173,
  [177] = 177,
  [178] = 178,
  [179] = 179,
  [180] = 180,
  [181] = 181,
  [182] = 182,
  [183] = 183,
  [184] = 184,
  [185] = 185,
  [186] = 186,
  [187] = 187,
  [188] = 185,
  [189] = 189,
  [190] = 190,
  [191] = 191,
  [192] = 192,
  [193] = 193,
  [194] = 194,
  [195] = 195,
  [196] = 196,
  [197] = 197,
  [198] = 198,
  [199] = 199,
  [200] = 200,
  [201] = 201,
  [202] = 202,
  [203] = 203,
  [204] = 204,
  [205] = 205,
  [206] = 202,
  [207] = 207,
  [208] = 208,
  [209] = 207,
  [210] = 200,
  [211] = 208,
  [212] = 212,
  [213] = 213,
  [214] = 214,
  [215] = 204,
  [216] = 203,
  [217] = 213,
  [218] = 212,
  [219] = 202,
  [220] = 220,
  [221] = 221,
  [222] = 222,
  [223] = 223,
  [224] = 224,
  [225] = 225,
  [226] = 226,
  [227] = 227,
  [228] = 228,
  [229] = 229,
  [230] = 230,
  [231] = 230,
  [232] = 232,
  [233] = 230,
  [234] = 230,
  [235] = 230,
  [236] = 236,
  [237] = 237,
  [238] = 230,
  [239] = 239,
  [240] = 224,
  [241] = 230,
  [242] = 242,
  [243] = 243,
  [244] = 244,
  [245] = 245,
  [246] = 246,
  [247] = 247,
  [248] = 248,
  [249] = 249,
  [250] = 250,
  [251] = 251,
  [252] = 252,
  [253] = 253,
  [254] = 254,
  [255] = 247,
  [256] = 256,
  [257] = 257,
  [258] = 258,
  [259] = 259,
  [260] = 260,
  [261] = 261,
  [262] = 262,
  [263] = 263,
  [264] = 264,
  [265] = 265,
  [266] = 266,
  [267] = 267,
  [268] = 268,
  [269] = 269,
  [270] = 270,
  [271] = 271,
  [272] = 272,
  [273] = 273,
  [274] = 274,
  [275] = 275,
  [276] = 276,
  [277] = 277,
  [278] = 278,
  [279] = 279,
  [280] = 280,
  [281] = 281,
  [282] = 282,
  [283] = 283,
  [284] = 284,
  [285] = 285,
  [286] = 286,
  [287] = 287,
  [288] = 288,
  [289] = 289,
  [290] = 290,
  [291] = 291,
  [292] = 292,
  [293] = 293,
  [294] = 294,
  [295] = 295,
  [296] = 296,
  [297] = 297,
  [298] = 298,
  [299] = 299,
  [300] = 297,
  [301] = 301,
  [302] = 302,
  [303] = 303,
  [304] = 304,
  [305] = 288,
  [306] = 306,
  [307] = 287,
  [308] = 276,
  [309] = 309,
  [310] = 310,
  [311] = 311,
  [312] = 297,
  [313] = 313,
  [314] = 314,
  [315] = 315,
  [316] = 288,
  [317] = 317,
  [318] = 287,
  [319] = 276,
  [320] = 320,
  [321] = 321,
  [322] = 322,
  [323] = 323,
  [324] = 324,
  [325] = 317,
  [326] = 326,
  [327] = 262,
  [328] = 328,
  [329] = 329,
  [330] = 317,
  [331] = 331,
  [332] = 262,
  [333] = 324,
  [334] = 324,
};

static bool ts_lex(TSLexer *lexer, TSStateId state) {
  START_LEXER();
  eof = lexer->eof(lexer);
  switch (state) {
    case 0:
      if (eof) ADVANCE(228);
      ADVANCE_MAP(
        '"', 348,
        '&', 329,
        '\'', 230,
        '(', 259,
        ')', 260,
        '*', 325,
        '+', 323,
        ',', 272,
        '-', 324,
        '.', 322,
        '/', 326,
        ':', 31,
        '<', 337,
        '=', 562,
        '>', 338,
        'A', 129,
        'B', 168,
        'C', 40,
        'D', 41,
        'E', 130,
        'F', 42,
        'I', 101,
        'L', 113,
        'M', 167,
        'N', 74,
        'O', 57,
        'P', 47,
        'R', 75,
        'S', 118,
        'T', 111,
        'U', 158,
        'V', 49,
        'W', 97,
      );
      if (lookahead == '_') SKIP(225);
      if (lookahead == 'a') ADVANCE(156);
      if (lookahead == 'o') ADVANCE(180);
      if (lookahead == '\t' ||
          lookahead == ' ') ADVANCE(575);
      if (lookahead == 'X' ||
          lookahead == 'x') ADVANCE(171);
      if (('0' <= lookahead && lookahead <= '9')) ADVANCE(346);
      END_STATE();
    case 1:
      if (lookahead == ' ') ADVANCE(35);
      END_STATE();
    case 2:
      ADVANCE_MAP(
        '"', 348,
        '&', 329,
        '\'', 230,
        '(', 259,
        '*', 325,
        '+', 323,
        ',', 272,
        '-', 324,
        '.', 322,
        '/', 326,
        '<', 337,
        '=', 562,
        '>', 338,
        'C', 388,
        'F', 391,
        'M', 503,
        'N', 419,
        'T', 519,
        '_', 380,
        '\t', 575,
        ' ', 575,
        '\n', 563,
        '\r', 563,
        'A', 492,
        'a', 492,
        'O', 517,
        'o', 517,
        'X', 508,
        'x', 508,
      );
      if (('0' <= lookahead && lookahead <= '9')) ADVANCE(346);
      if (('B' <= lookahead && lookahead <= 'Z') ||
          ('b' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 3:
      ADVANCE_MAP(
        '"', 348,
        '\'', 230,
        '(', 259,
        ')', 260,
        '-', 324,
        'C', 388,
        'F', 391,
        'N', 419,
        'T', 519,
        '_', 382,
        '\t', 575,
        ' ', 575,
      );
      if (('0' <= lookahead && lookahead <= '9')) ADVANCE(346);
      if (('A' <= lookahead && lookahead <= 'Z') ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 4:
      ADVANCE_MAP(
        '"', 348,
        '\'', 230,
        '(', 259,
        '-', 324,
        'A', 483,
        'B', 502,
        'C', 387,
        'D', 403,
        'F', 391,
        'I', 498,
        'L', 506,
        'N', 419,
        'O', 405,
        'S', 458,
        'T', 519,
        'V', 402,
        '_', 360,
        '\t', 575,
        ' ', 575,
      );
      if (('0' <= lookahead && lookahead <= '9')) ADVANCE(346);
      if (('E' <= lookahead && lookahead <= 'Z') ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 5:
      ADVANCE_MAP(
        '"', 348,
        '\'', 230,
        '(', 259,
        '-', 324,
        'C', 388,
        'D', 450,
        'E', 555,
        'F', 390,
        'I', 442,
        'L', 504,
        'N', 419,
        'R', 420,
        'T', 519,
        'W', 449,
        '_', 365,
        '\t', 575,
        ' ', 575,
      );
      if (('0' <= lookahead && lookahead <= '9')) ADVANCE(346);
      if (('A' <= lookahead && lookahead <= 'Z') ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 6:
      ADVANCE_MAP(
        '"', 348,
        '\'', 230,
        '(', 259,
        '-', 324,
        'C', 388,
        'D', 450,
        'E', 555,
        'F', 390,
        'I', 442,
        'L', 504,
        'N', 419,
        'R', 420,
        'T', 519,
        'W', 449,
        '_', 366,
        '\t', 575,
        ' ', 575,
        '\n', 563,
        '\r', 563,
      );
      if (('0' <= lookahead && lookahead <= '9')) ADVANCE(346);
      if (('A' <= lookahead && lookahead <= 'Z') ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 7:
      ADVANCE_MAP(
        '"', 348,
        '\'', 230,
        '(', 259,
        '-', 324,
        'C', 388,
        'D', 450,
        'E', 555,
        'F', 390,
        'I', 442,
        'N', 419,
        'R', 420,
        'T', 519,
        'W', 449,
        '_', 367,
        '\t', 575,
        ' ', 575,
        '\n', 563,
        '\r', 563,
      );
      if (('0' <= lookahead && lookahead <= '9')) ADVANCE(346);
      if (('A' <= lookahead && lookahead <= 'Z') ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 8:
      ADVANCE_MAP(
        '"', 348,
        '\'', 230,
        '(', 259,
        '-', 324,
        'C', 388,
        'D', 450,
        'E', 555,
        'F', 390,
        'I', 442,
        'N', 419,
        'R', 420,
        'T', 519,
        'W', 436,
        '_', 368,
        '\t', 575,
        ' ', 575,
      );
      if (('0' <= lookahead && lookahead <= '9')) ADVANCE(346);
      if (('A' <= lookahead && lookahead <= 'Z') ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 9:
      ADVANCE_MAP(
        '"', 348,
        '\'', 230,
        '(', 259,
        '-', 324,
        'C', 388,
        'D', 450,
        'E', 555,
        'F', 390,
        'I', 442,
        'N', 419,
        'R', 420,
        'T', 519,
        'W', 436,
        '_', 369,
        '\t', 575,
        ' ', 575,
        '\n', 563,
        '\r', 563,
      );
      if (('0' <= lookahead && lookahead <= '9')) ADVANCE(346);
      if (('A' <= lookahead && lookahead <= 'Z') ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 10:
      ADVANCE_MAP(
        '"', 348,
        '\'', 230,
        '(', 259,
        '-', 324,
        'C', 388,
        'D', 450,
        'E', 555,
        'F', 390,
        'I', 442,
        'N', 430,
        'R', 420,
        'T', 519,
        'W', 449,
        '_', 370,
        '\t', 575,
        ' ', 575,
      );
      if (('0' <= lookahead && lookahead <= '9')) ADVANCE(346);
      if (('A' <= lookahead && lookahead <= 'Z') ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 11:
      ADVANCE_MAP(
        '"', 348,
        '\'', 230,
        '(', 259,
        '-', 324,
        'C', 388,
        'D', 450,
        'E', 555,
        'F', 390,
        'I', 442,
        'N', 430,
        'R', 420,
        'T', 519,
        'W', 449,
        '_', 371,
        '\t', 575,
        ' ', 575,
        '\n', 563,
        '\r', 563,
      );
      if (('0' <= lookahead && lookahead <= '9')) ADVANCE(346);
      if (('A' <= lookahead && lookahead <= 'Z') ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 12:
      ADVANCE_MAP(
        '"', 348,
        '\'', 230,
        '(', 259,
        '-', 324,
        'C', 388,
        'D', 450,
        'E', 474,
        'F', 390,
        'I', 442,
        'N', 419,
        'R', 420,
        'T', 519,
        'W', 449,
        '_', 372,
        '\t', 575,
        ' ', 575,
      );
      if (('0' <= lookahead && lookahead <= '9')) ADVANCE(346);
      if (('A' <= lookahead && lookahead <= 'Z') ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 13:
      ADVANCE_MAP(
        '"', 348,
        '\'', 230,
        '(', 259,
        '-', 324,
        'C', 388,
        'D', 450,
        'E', 474,
        'F', 390,
        'I', 442,
        'N', 419,
        'R', 420,
        'T', 519,
        'W', 449,
        '_', 373,
        '\t', 575,
        ' ', 575,
        '\n', 563,
        '\r', 563,
      );
      if (('0' <= lookahead && lookahead <= '9')) ADVANCE(346);
      if (('A' <= lookahead && lookahead <= 'Z') ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 14:
      ADVANCE_MAP(
        '"', 348,
        '\'', 230,
        '(', 259,
        '-', 324,
        'C', 388,
        'D', 450,
        'E', 496,
        'F', 390,
        'I', 442,
        'N', 419,
        'R', 420,
        'T', 519,
        'W', 449,
        '_', 374,
        '\t', 575,
        ' ', 575,
      );
      if (('0' <= lookahead && lookahead <= '9')) ADVANCE(346);
      if (('A' <= lookahead && lookahead <= 'Z') ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 15:
      ADVANCE_MAP(
        '"', 348,
        '\'', 230,
        '(', 259,
        '-', 324,
        'C', 388,
        'D', 450,
        'E', 496,
        'F', 390,
        'I', 442,
        'N', 419,
        'R', 420,
        'T', 519,
        'W', 449,
        '_', 375,
        '\t', 575,
        ' ', 575,
        '\n', 563,
        '\r', 563,
      );
      if (('0' <= lookahead && lookahead <= '9')) ADVANCE(346);
      if (('A' <= lookahead && lookahead <= 'Z') ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 16:
      ADVANCE_MAP(
        '"', 348,
        '\'', 230,
        '(', 259,
        '-', 324,
        'C', 388,
        'D', 450,
        'E', 487,
        'F', 390,
        'I', 442,
        'N', 419,
        'R', 420,
        'T', 519,
        'W', 449,
        '_', 376,
        '\t', 575,
        ' ', 575,
        '\n', 563,
        '\r', 563,
      );
      if (('0' <= lookahead && lookahead <= '9')) ADVANCE(346);
      if (('A' <= lookahead && lookahead <= 'Z') ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 17:
      ADVANCE_MAP(
        '"', 348,
        '\'', 230,
        '(', 259,
        '-', 324,
        'C', 388,
        'D', 450,
        'E', 487,
        'F', 390,
        'I', 442,
        'N', 419,
        'R', 420,
        'T', 519,
        'W', 449,
        '_', 377,
        '\t', 575,
        ' ', 575,
      );
      if (('0' <= lookahead && lookahead <= '9')) ADVANCE(346);
      if (('A' <= lookahead && lookahead <= 'Z') ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 18:
      ADVANCE_MAP(
        '"', 348,
        '\'', 230,
        '(', 259,
        '-', 324,
        'C', 388,
        'D', 450,
        'E', 497,
        'F', 390,
        'I', 442,
        'N', 419,
        'R', 420,
        'T', 519,
        'W', 449,
        '_', 378,
        '\t', 575,
        ' ', 575,
        '\n', 563,
        '\r', 563,
      );
      if (('0' <= lookahead && lookahead <= '9')) ADVANCE(346);
      if (('A' <= lookahead && lookahead <= 'Z') ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 19:
      ADVANCE_MAP(
        '"', 348,
        '\'', 230,
        '(', 259,
        '-', 324,
        'C', 388,
        'D', 450,
        'E', 497,
        'F', 390,
        'I', 442,
        'N', 419,
        'R', 420,
        'T', 519,
        'W', 449,
        '_', 379,
        '\t', 575,
        ' ', 575,
      );
      if (('0' <= lookahead && lookahead <= '9')) ADVANCE(346);
      if (('A' <= lookahead && lookahead <= 'Z') ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 20:
      ADVANCE_MAP(
        '"', 348,
        '\'', 230,
        '(', 259,
        '-', 324,
        'C', 388,
        'F', 391,
        'N', 419,
        'P', 530,
        'T', 519,
        '_', 381,
        '\t', 575,
        ' ', 575,
      );
      if (('0' <= lookahead && lookahead <= '9')) ADVANCE(346);
      if (('A' <= lookahead && lookahead <= 'Z') ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 21:
      ADVANCE_MAP(
        '&', 329,
        '\'', 230,
        '(', 259,
        '*', 325,
        '+', 323,
        ',', 272,
        '-', 324,
        '.', 322,
        '/', 326,
        ':', 31,
        '<', 337,
        '=', 562,
        '>', 338,
        'A', 157,
        'M', 167,
        'S', 209,
      );
      if (lookahead == '_') SKIP(22);
      if (lookahead == 'a') ADVANCE(156);
      if (lookahead == '\t' ||
          lookahead == ' ') ADVANCE(575);
      if (lookahead == '\n' ||
          lookahead == '\r') ADVANCE(563);
      if (lookahead == 'O' ||
          lookahead == 'o') ADVANCE(180);
      if (lookahead == 'X' ||
          lookahead == 'x') ADVANCE(171);
      END_STATE();
    case 22:
      ADVANCE_MAP(
        '&', 329,
        '\'', 230,
        '(', 259,
        '*', 325,
        '+', 323,
        ',', 272,
        '-', 324,
        '.', 322,
        '/', 326,
        ':', 31,
        '<', 337,
        '=', 562,
        '>', 338,
        'A', 157,
        'M', 167,
        'S', 209,
      );
      if (lookahead == '_') SKIP(22);
      if (lookahead == 'a') ADVANCE(156);
      if (lookahead == '\t' ||
          lookahead == ' ') ADVANCE(575);
      if (lookahead == '\n' ||
          lookahead == '\r') ADVANCE(574);
      if (lookahead == 'O' ||
          lookahead == 'o') ADVANCE(180);
      if (lookahead == 'X' ||
          lookahead == 'x') ADVANCE(171);
      END_STATE();
    case 23:
      ADVANCE_MAP(
        '&', 329,
        '\'', 230,
        '(', 30,
        ')', 260,
        '*', 325,
        '+', 323,
        ',', 272,
        '-', 324,
        '.', 322,
        '/', 326,
        '<', 337,
        '=', 562,
        '>', 338,
        'D', 166,
        'F', 169,
        'M', 167,
        'S', 215,
        'T', 110,
        'W', 112,
      );
      if (lookahead == '_') SKIP(23);
      if (lookahead == '\t' ||
          lookahead == ' ') ADVANCE(575);
      if (lookahead == '\n' ||
          lookahead == '\r') SKIP(23);
      if (lookahead == 'A' ||
          lookahead == 'a') ADVANCE(156);
      if (lookahead == 'O' ||
          lookahead == 'o') ADVANCE(180);
      if (lookahead == 'X' ||
          lookahead == 'x') ADVANCE(171);
      END_STATE();
    case 24:
      ADVANCE_MAP(
        '&', 329,
        '\'', 230,
        '(', 30,
        ')', 260,
        '*', 325,
        '+', 323,
        ',', 272,
        '-', 324,
        '.', 322,
        '/', 326,
        '<', 337,
        '=', 562,
        '>', 338,
        'D', 166,
        'F', 169,
        'M', 167,
        'S', 215,
        'T', 110,
        'W', 112,
      );
      if (lookahead == '_') SKIP(23);
      if (lookahead == '\t' ||
          lookahead == ' ') ADVANCE(575);
      if (lookahead == 'A' ||
          lookahead == 'a') ADVANCE(156);
      if (lookahead == 'O' ||
          lookahead == 'o') ADVANCE(180);
      if (lookahead == 'X' ||
          lookahead == 'x') ADVANCE(171);
      END_STATE();
    case 25:
      ADVANCE_MAP(
        '&', 329,
        '\'', 230,
        '(', 30,
        '*', 325,
        '+', 323,
        ',', 272,
        '-', 324,
        '.', 322,
        '/', 326,
        '<', 337,
        '=', 562,
        '>', 338,
        'M', 167,
        'S', 209,
      );
      if (lookahead == '_') SKIP(26);
      if (lookahead == '\t' ||
          lookahead == ' ') ADVANCE(575);
      if (lookahead == '\n' ||
          lookahead == '\r') ADVANCE(563);
      if (lookahead == 'A' ||
          lookahead == 'a') ADVANCE(156);
      if (lookahead == 'O' ||
          lookahead == 'o') ADVANCE(180);
      if (lookahead == 'X' ||
          lookahead == 'x') ADVANCE(171);
      END_STATE();
    case 26:
      ADVANCE_MAP(
        '&', 329,
        '\'', 230,
        '(', 30,
        '*', 325,
        '+', 323,
        ',', 272,
        '-', 324,
        '.', 322,
        '/', 326,
        '<', 337,
        '=', 562,
        '>', 338,
        'M', 167,
        'S', 209,
      );
      if (lookahead == '_') SKIP(26);
      if (lookahead == '\t' ||
          lookahead == ' ') ADVANCE(575);
      if (lookahead == '\n' ||
          lookahead == '\r') ADVANCE(574);
      if (lookahead == 'A' ||
          lookahead == 'a') ADVANCE(156);
      if (lookahead == 'O' ||
          lookahead == 'o') ADVANCE(180);
      if (lookahead == 'X' ||
          lookahead == 'x') ADVANCE(171);
      END_STATE();
    case 27:
      ADVANCE_MAP(
        '\'', 230,
        ')', 260,
        'B', 559,
        'O', 513,
        'P', 400,
        '_', 362,
        '\t', 575,
        ' ', 575,
      );
      if (('A' <= lookahead && lookahead <= 'Z') ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 28:
      if (lookahead == '\'') ADVANCE(230);
      if (lookahead == 'C') ADVANCE(388);
      if (lookahead == '_') ADVANCE(383);
      if (lookahead == '\t' ||
          lookahead == ' ') ADVANCE(575);
      if (('A' <= lookahead && lookahead <= 'Z') ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 29:
      if (lookahead == '\'') ADVANCE(230);
      if (lookahead == '_') ADVANCE(386);
      if (lookahead == '\t' ||
          lookahead == ' ') ADVANCE(575);
      if (('A' <= lookahead && lookahead <= 'Z') ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 30:
      if (lookahead == ')') ADVANCE(319);
      END_STATE();
    case 31:
      if (lookahead == '=') ADVANCE(345);
      END_STATE();
    case 32:
      if (lookahead == 'A') ADVANCE(191);
      END_STATE();
    case 33:
      if (lookahead == 'D') ADVANCE(116);
      END_STATE();
    case 34:
      if (lookahead == 'F') ADVANCE(218);
      END_STATE();
    case 35:
      if (lookahead == 'F') ADVANCE(218);
      if (lookahead == 'I') ADVANCE(103);
      if (lookahead == 'S') ADVANCE(216);
      END_STATE();
    case 36:
      if (lookahead == 'I') ADVANCE(103);
      END_STATE();
    case 37:
      if (lookahead == 'R') ADVANCE(76);
      if (lookahead == 'V') ADVANCE(48);
      if (lookahead == 't') ADVANCE(77);
      END_STATE();
    case 38:
      if (lookahead == 'S') ADVANCE(216);
      END_STATE();
    case 39:
      if (lookahead == 'S') ADVANCE(45);
      END_STATE();
    case 40:
      if (lookahead == 'a') ADVANCE(136);
      if (lookahead == 'o') ADVANCE(142);
      if (lookahead == 'u') ADVANCE(189);
      END_STATE();
    case 41:
      if (lookahead == 'a') ADVANCE(205);
      if (lookahead == 'e') ADVANCE(62);
      if (lookahead == 'i') ADVANCE(65);
      if (lookahead == 'o') ADVANCE(240);
      END_STATE();
    case 42:
      if (lookahead == 'a') ADVANCE(141);
      if (lookahead == 'o') ADVANCE(181);
      if (lookahead == 'u') ADVANCE(153);
      END_STATE();
    case 43:
      if (lookahead == 'a') ADVANCE(195);
      END_STATE();
    case 44:
      if (lookahead == 'a') ADVANCE(144);
      END_STATE();
    case 45:
      if (lookahead == 'a') ADVANCE(104);
      END_STATE();
    case 46:
      if (lookahead == 'a') ADVANCE(224);
      END_STATE();
    case 47:
      if (lookahead == 'a') ADVANCE(188);
      if (lookahead == 'r') ADVANCE(99);
      if (lookahead == 't') ADVANCE(182);
      END_STATE();
    case 48:
      if (lookahead == 'a') ADVANCE(132);
      END_STATE();
    case 49:
      if (lookahead == 'a') ADVANCE(193);
      END_STATE();
    case 50:
      if (lookahead == 'a') ADVANCE(134);
      END_STATE();
    case 51:
      if (lookahead == 'a') ADVANCE(135);
      END_STATE();
    case 52:
      if (lookahead == 'a') ADVANCE(149);
      END_STATE();
    case 53:
      if (lookahead == 'a') ADVANCE(192);
      END_STATE();
    case 54:
      if (lookahead == 'a') ADVANCE(187);
      END_STATE();
    case 55:
      if (lookahead == 'a') ADVANCE(162);
      END_STATE();
    case 56:
      if (lookahead == 'a') ADVANCE(208);
      END_STATE();
    case 57:
      if (lookahead == 'b') ADVANCE(128);
      if (lookahead == 'p') ADVANCE(204);
      if (lookahead == 'r') ADVANCE(332);
      END_STATE();
    case 58:
      if (lookahead == 'b') ADVANCE(275);
      END_STATE();
    case 59:
      if (lookahead == 'b') ADVANCE(237);
      END_STATE();
    case 60:
      if (lookahead == 'b') ADVANCE(261);
      END_STATE();
    case 61:
      if (lookahead == 'b') ADVANCE(139);
      END_STATE();
    case 62:
      if (lookahead == 'c') ADVANCE(114);
      END_STATE();
    case 63:
      if (lookahead == 'c') ADVANCE(222);
      END_STATE();
    case 64:
      if (lookahead == 'c') ADVANCE(202);
      END_STATE();
    case 65:
      if (lookahead == 'c') ADVANCE(211);
      if (lookahead == 'm') ADVANCE(265);
      END_STATE();
    case 66:
      if (lookahead == 'c') ADVANCE(212);
      END_STATE();
    case 67:
      if (lookahead == 'c') ADVANCE(213);
      END_STATE();
    case 68:
      if (lookahead == 'c') ADVANCE(214);
      END_STATE();
    case 69:
      if (lookahead == 'd') ADVANCE(330);
      END_STATE();
    case 70:
      if (lookahead == 'd') ADVANCE(330);
      if (lookahead == 'y') ADVANCE(285);
      END_STATE();
    case 71:
      if (lookahead == 'd') ADVANCE(1);
      END_STATE();
    case 72:
      if (lookahead == 'd') ADVANCE(327);
      END_STATE();
    case 73:
      if (lookahead == 'd') ADVANCE(254);
      END_STATE();
    case 74:
      if (lookahead == 'e') ADVANCE(221);
      if (lookahead == 'o') ADVANCE(199);
      END_STATE();
    case 75:
      if (lookahead == 'e') ADVANCE(33);
      END_STATE();
    case 76:
      if (lookahead == 'e') ADVANCE(102);
      END_STATE();
    case 77:
      if (lookahead == 'e') ADVANCE(289);
      END_STATE();
    case 78:
      if (lookahead == 'e') ADVANCE(295);
      END_STATE();
    case 79:
      if (lookahead == 'e') ADVANCE(247);
      END_STATE();
    case 80:
      if (lookahead == 'e') ADVANCE(353);
      END_STATE();
    case 81:
      if (lookahead == 'e') ADVANCE(355);
      END_STATE();
    case 82:
      if (lookahead == 'e') ADVANCE(242);
      END_STATE();
    case 83:
      if (lookahead == 'e') ADVANCE(301);
      END_STATE();
    case 84:
      if (lookahead == 'e') ADVANCE(313);
      END_STATE();
    case 85:
      if (lookahead == 'e') ADVANCE(273);
      END_STATE();
    case 86:
      if (lookahead == 'e') ADVANCE(262);
      END_STATE();
    case 87:
      if (lookahead == 'e') ADVANCE(274);
      END_STATE();
    case 88:
      if (lookahead == 'e') ADVANCE(269);
      END_STATE();
    case 89:
      if (lookahead == 'e') ADVANCE(179);
      END_STATE();
    case 90:
      if (lookahead == 'e') ADVANCE(179);
      if (lookahead == 'r') ADVANCE(120);
      END_STATE();
    case 91:
      if (lookahead == 'e') ADVANCE(108);
      END_STATE();
    case 92:
      if (lookahead == 'e') ADVANCE(148);
      END_STATE();
    case 93:
      if (lookahead == 'e') ADVANCE(155);
      END_STATE();
    case 94:
      if (lookahead == 'e') ADVANCE(52);
      END_STATE();
    case 95:
      if (lookahead == 'e') ADVANCE(186);
      END_STATE();
    case 96:
      if (lookahead == 'e') ADVANCE(184);
      END_STATE();
    case 97:
      if (lookahead == 'e') ADVANCE(159);
      if (lookahead == 'h') ADVANCE(124);
      END_STATE();
    case 98:
      if (lookahead == 'e') ADVANCE(64);
      END_STATE();
    case 99:
      if (lookahead == 'e') ADVANCE(198);
      if (lookahead == 'i') ADVANCE(219);
      END_STATE();
    case 100:
      if (lookahead == 'e') ADVANCE(67);
      END_STATE();
    case 101:
      if (lookahead == 'f') ADVANCE(244);
      if (lookahead == 'n') ADVANCE(206);
      END_STATE();
    case 102:
      if (lookahead == 'f') ADVANCE(279);
      END_STATE();
    case 103:
      if (lookahead == 'f') ADVANCE(249);
      END_STATE();
    case 104:
      if (lookahead == 'f') ADVANCE(87);
      END_STATE();
    case 105:
      if (lookahead == 'g') ADVANCE(305);
      END_STATE();
    case 106:
      if (lookahead == 'g') ADVANCE(315);
      END_STATE();
    case 107:
      if (lookahead == 'g') ADVANCE(307);
      END_STATE();
    case 108:
      if (lookahead == 'g') ADVANCE(96);
      END_STATE();
    case 109:
      if (lookahead == 'g') ADVANCE(140);
      END_STATE();
    case 110:
      if (lookahead == 'h') ADVANCE(92);
      if (lookahead == 'o') ADVANCE(250);
      END_STATE();
    case 111:
      if (lookahead == 'h') ADVANCE(92);
      if (lookahead == 'o') ADVANCE(250);
      if (lookahead == 'r') ADVANCE(217);
      END_STATE();
    case 112:
      if (lookahead == 'h') ADVANCE(124);
      END_STATE();
    case 113:
      if (lookahead == 'i') ADVANCE(58);
      if (lookahead == 'o') ADVANCE(147);
      END_STATE();
    case 114:
      if (lookahead == 'i') ADVANCE(146);
      if (lookahead == 'l') ADVANCE(53);
      END_STATE();
    case 115:
      if (lookahead == 'i') ADVANCE(43);
      END_STATE();
    case 116:
      if (lookahead == 'i') ADVANCE(145);
      END_STATE();
    case 117:
      if (lookahead == 'i') ADVANCE(172);
      END_STATE();
    case 118:
      if (lookahead == 'i') ADVANCE(154);
      if (lookahead == 't') ADVANCE(90);
      if (lookahead == 'u') ADVANCE(59);
      END_STATE();
    case 119:
      if (lookahead == 'i') ADVANCE(200);
      END_STATE();
    case 120:
      if (lookahead == 'i') ADVANCE(160);
      END_STATE();
    case 121:
      if (lookahead == 'i') ADVANCE(133);
      END_STATE();
    case 122:
      if (lookahead == 'i') ADVANCE(55);
      END_STATE();
    case 123:
      if (lookahead == 'i') ADVANCE(173);
      END_STATE();
    case 124:
      if (lookahead == 'i') ADVANCE(138);
      END_STATE();
    case 125:
      if (lookahead == 'i') ADVANCE(175);
      END_STATE();
    case 126:
      if (lookahead == 'i') ADVANCE(176);
      END_STATE();
    case 127:
      if (lookahead == 'i') ADVANCE(177);
      END_STATE();
    case 128:
      if (lookahead == 'j') ADVANCE(98);
      END_STATE();
    case 129:
      if (lookahead == 'l') ADVANCE(115);
      if (lookahead == 'n') ADVANCE(70);
      if (lookahead == 's') ADVANCE(271);
      END_STATE();
    case 130:
      if (lookahead == 'l') ADVANCE(196);
      if (lookahead == 'n') ADVANCE(71);
      if (lookahead == 'x') ADVANCE(119);
      END_STATE();
    case 131:
      if (lookahead == 'l') ADVANCE(343);
      END_STATE();
    case 132:
      if (lookahead == 'l') ADVANCE(277);
      END_STATE();
    case 133:
      if (lookahead == 'l') ADVANCE(258);
      END_STATE();
    case 134:
      if (lookahead == 'l') ADVANCE(297);
      END_STATE();
    case 135:
      if (lookahead == 'l') ADVANCE(281);
      END_STATE();
    case 136:
      if (lookahead == 'l') ADVANCE(131);
      END_STATE();
    case 137:
      if (lookahead == 'l') ADVANCE(94);
      END_STATE();
    case 138:
      if (lookahead == 'l') ADVANCE(82);
      END_STATE();
    case 139:
      if (lookahead == 'l') ADVANCE(83);
      END_STATE();
    case 140:
      if (lookahead == 'l') ADVANCE(84);
      END_STATE();
    case 141:
      if (lookahead == 'l') ADVANCE(197);
      END_STATE();
    case 142:
      if (lookahead == 'l') ADVANCE(143);
      END_STATE();
    case 143:
      if (lookahead == 'l') ADVANCE(100);
      END_STATE();
    case 144:
      if (lookahead == 'm') ADVANCE(32);
      END_STATE();
    case 145:
      if (lookahead == 'm') ADVANCE(267);
      END_STATE();
    case 146:
      if (lookahead == 'm') ADVANCE(50);
      END_STATE();
    case 147:
      if (lookahead == 'n') ADVANCE(105);
      if (lookahead == 'o') ADVANCE(178);
      END_STATE();
    case 148:
      if (lookahead == 'n') ADVANCE(246);
      END_STATE();
    case 149:
      if (lookahead == 'n') ADVANCE(287);
      END_STATE();
    case 150:
      if (lookahead == 'n') ADVANCE(235);
      END_STATE();
    case 151:
      if (lookahead == 'n') ADVANCE(291);
      END_STATE();
    case 152:
      if (lookahead == 'n') ADVANCE(264);
      END_STATE();
    case 153:
      if (lookahead == 'n') ADVANCE(66);
      END_STATE();
    case 154:
      if (lookahead == 'n') ADVANCE(109);
      END_STATE();
    case 155:
      if (lookahead == 'n') ADVANCE(63);
      END_STATE();
    case 156:
      if (lookahead == 'n') ADVANCE(69);
      END_STATE();
    case 157:
      if (lookahead == 'n') ADVANCE(69);
      if (lookahead == 's') ADVANCE(271);
      END_STATE();
    case 158:
      if (lookahead == 'n') ADVANCE(210);
      END_STATE();
    case 159:
      if (lookahead == 'n') ADVANCE(73);
      END_STATE();
    case 160:
      if (lookahead == 'n') ADVANCE(106);
      END_STATE();
    case 161:
      if (lookahead == 'n') ADVANCE(107);
      END_STATE();
    case 162:
      if (lookahead == 'n') ADVANCE(203);
      END_STATE();
    case 163:
      if (lookahead == 'n') ADVANCE(54);
      END_STATE();
    case 164:
      if (lookahead == 'n') ADVANCE(51);
      END_STATE();
    case 165:
      if (lookahead == 'n') ADVANCE(68);
      END_STATE();
    case 166:
      if (lookahead == 'o') ADVANCE(239);
      END_STATE();
    case 167:
      if (lookahead == 'o') ADVANCE(72);
      END_STATE();
    case 168:
      if (lookahead == 'o') ADVANCE(170);
      if (lookahead == 'y') ADVANCE(37);
      END_STATE();
    case 169:
      if (lookahead == 'o') ADVANCE(181);
      if (lookahead == 'u') ADVANCE(153);
      END_STATE();
    case 170:
      if (lookahead == 'o') ADVANCE(137);
      END_STATE();
    case 171:
      if (lookahead == 'o') ADVANCE(183);
      END_STATE();
    case 172:
      if (lookahead == 'o') ADVANCE(164);
      END_STATE();
    case 173:
      if (lookahead == 'o') ADVANCE(163);
      END_STATE();
    case 174:
      if (lookahead == 'o') ADVANCE(161);
      END_STATE();
    case 175:
      if (lookahead == 'o') ADVANCE(150);
      END_STATE();
    case 176:
      if (lookahead == 'o') ADVANCE(151);
      END_STATE();
    case 177:
      if (lookahead == 'o') ADVANCE(152);
      END_STATE();
    case 178:
      if (lookahead == 'p') ADVANCE(256);
      END_STATE();
    case 179:
      if (lookahead == 'p') ADVANCE(251);
      END_STATE();
    case 180:
      if (lookahead == 'r') ADVANCE(332);
      END_STATE();
    case 181:
      if (lookahead == 'r') ADVANCE(233);
      END_STATE();
    case 182:
      if (lookahead == 'r') ADVANCE(39);
      END_STATE();
    case 183:
      if (lookahead == 'r') ADVANCE(334);
      END_STATE();
    case 184:
      if (lookahead == 'r') ADVANCE(303);
      END_STATE();
    case 185:
      if (lookahead == 'r') ADVANCE(309);
      END_STATE();
    case 186:
      if (lookahead == 'r') ADVANCE(220);
      END_STATE();
    case 187:
      if (lookahead == 'r') ADVANCE(223);
      END_STATE();
    case 188:
      if (lookahead == 'r') ADVANCE(44);
      END_STATE();
    case 189:
      if (lookahead == 'r') ADVANCE(194);
      END_STATE();
    case 190:
      if (lookahead == 'r') ADVANCE(46);
      END_STATE();
    case 191:
      if (lookahead == 'r') ADVANCE(190);
      END_STATE();
    case 192:
      if (lookahead == 'r') ADVANCE(85);
      END_STATE();
    case 193:
      if (lookahead == 'r') ADVANCE(122);
      END_STATE();
    case 194:
      if (lookahead == 'r') ADVANCE(93);
      END_STATE();
    case 195:
      if (lookahead == 's') ADVANCE(276);
      END_STATE();
    case 196:
      if (lookahead == 's') ADVANCE(79);
      END_STATE();
    case 197:
      if (lookahead == 's') ADVANCE(81);
      END_STATE();
    case 198:
      if (lookahead == 's') ADVANCE(95);
      END_STATE();
    case 199:
      if (lookahead == 't') ADVANCE(341);
      END_STATE();
    case 200:
      if (lookahead == 't') ADVANCE(231);
      END_STATE();
    case 201:
      if (lookahead == 't') ADVANCE(252);
      END_STATE();
    case 202:
      if (lookahead == 't') ADVANCE(311);
      END_STATE();
    case 203:
      if (lookahead == 't') ADVANCE(317);
      END_STATE();
    case 204:
      if (lookahead == 't') ADVANCE(117);
      END_STATE();
    case 205:
      if (lookahead == 't') ADVANCE(78);
      END_STATE();
    case 206:
      if (lookahead == 't') ADVANCE(91);
      END_STATE();
    case 207:
      if (lookahead == 't') ADVANCE(185);
      END_STATE();
    case 208:
      if (lookahead == 't') ADVANCE(86);
      END_STATE();
    case 209:
      if (lookahead == 't') ADVANCE(89);
      END_STATE();
    case 210:
      if (lookahead == 't') ADVANCE(121);
      END_STATE();
    case 211:
      if (lookahead == 't') ADVANCE(123);
      END_STATE();
    case 212:
      if (lookahead == 't') ADVANCE(125);
      END_STATE();
    case 213:
      if (lookahead == 't') ADVANCE(126);
      END_STATE();
    case 214:
      if (lookahead == 't') ADVANCE(127);
      END_STATE();
    case 215:
      if (lookahead == 'u') ADVANCE(59);
      END_STATE();
    case 216:
      if (lookahead == 'u') ADVANCE(60);
      END_STATE();
    case 217:
      if (lookahead == 'u') ADVANCE(80);
      END_STATE();
    case 218:
      if (lookahead == 'u') ADVANCE(165);
      END_STATE();
    case 219:
      if (lookahead == 'v') ADVANCE(56);
      END_STATE();
    case 220:
      if (lookahead == 'v') ADVANCE(88);
      END_STATE();
    case 221:
      if (lookahead == 'w') ADVANCE(320);
      if (lookahead == 'x') ADVANCE(201);
      END_STATE();
    case 222:
      if (lookahead == 'y') ADVANCE(293);
      END_STATE();
    case 223:
      if (lookahead == 'y') ADVANCE(299);
      END_STATE();
    case 224:
      if (lookahead == 'y') ADVANCE(283);
      END_STATE();
    case 225:
      if (eof) ADVANCE(228);
      ADVANCE_MAP(
        '"', 348,
        '&', 329,
        '\'', 230,
        '(', 259,
        ')', 260,
        '*', 325,
        '+', 323,
        ',', 272,
        '-', 324,
        '.', 322,
        '/', 326,
        ':', 31,
        '<', 337,
        '=', 562,
        '>', 338,
        'A', 129,
        'B', 168,
        'C', 40,
        'D', 41,
        'E', 130,
        'F', 42,
        'I', 101,
        'L', 113,
        'M', 167,
        'N', 74,
        'O', 57,
        'P', 47,
        'R', 75,
        'S', 118,
        'T', 111,
        'U', 158,
        'V', 49,
        'W', 97,
      );
      if (lookahead == '_') SKIP(225);
      if (lookahead == 'a') ADVANCE(156);
      if (lookahead == 'o') ADVANCE(180);
      if (lookahead == '\t' ||
          lookahead == ' ') ADVANCE(575);
      if (lookahead == '\n' ||
          lookahead == '\r') SKIP(225);
      if (lookahead == 'X' ||
          lookahead == 'x') ADVANCE(171);
      if (('0' <= lookahead && lookahead <= '9')) ADVANCE(346);
      END_STATE();
    case 226:
      if (eof) ADVANCE(228);
      ADVANCE_MAP(
        '"', 348,
        '\'', 230,
        '(', 259,
        '-', 324,
        'C', 388,
        'D', 450,
        'E', 555,
        'F', 389,
        'I', 442,
        'N', 419,
        'P', 520,
        'R', 420,
        'S', 548,
        'T', 519,
        'W', 449,
        '_', 363,
        '\t', 575,
        ' ', 575,
      );
      if (('0' <= lookahead && lookahead <= '9')) ADVANCE(346);
      if (('A' <= lookahead && lookahead <= 'Z') ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 227:
      if (eof) ADVANCE(228);
      ADVANCE_MAP(
        '"', 348,
        '\'', 230,
        '(', 259,
        '-', 324,
        'C', 388,
        'D', 450,
        'E', 555,
        'F', 389,
        'I', 442,
        'N', 419,
        'P', 520,
        'R', 420,
        'S', 548,
        'T', 519,
        'W', 449,
        '_', 364,
        '\t', 575,
        ' ', 575,
        '\n', 563,
        '\r', 563,
      );
      if (('0' <= lookahead && lookahead <= '9')) ADVANCE(346);
      if (('A' <= lookahead && lookahead <= 'Z') ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 228:
      ACCEPT_TOKEN(ts_builtin_sym_end);
      END_STATE();
    case 229:
      ACCEPT_TOKEN(sym_comment);
      if (lookahead == '\n') ADVANCE(352);
      if (lookahead == '"') ADVANCE(230);
      if (lookahead != 0) ADVANCE(229);
      END_STATE();
    case 230:
      ACCEPT_TOKEN(sym_comment);
      if (lookahead != 0 &&
          lookahead != '\n') ADVANCE(230);
      END_STATE();
    case 231:
      ACCEPT_TOKEN(anon_sym_Exit);
      END_STATE();
    case 232:
      ACCEPT_TOKEN(anon_sym_Exit);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 233:
      ACCEPT_TOKEN(anon_sym_For);
      END_STATE();
    case 234:
      ACCEPT_TOKEN(anon_sym_For);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 235:
      ACCEPT_TOKEN(anon_sym_Function);
      END_STATE();
    case 236:
      ACCEPT_TOKEN(anon_sym_Function);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 237:
      ACCEPT_TOKEN(anon_sym_Sub);
      END_STATE();
    case 238:
      ACCEPT_TOKEN(anon_sym_Sub);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 239:
      ACCEPT_TOKEN(anon_sym_Do);
      END_STATE();
    case 240:
      ACCEPT_TOKEN(anon_sym_Do);
      if (lookahead == 'u') ADVANCE(61);
      END_STATE();
    case 241:
      ACCEPT_TOKEN(anon_sym_Do);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 242:
      ACCEPT_TOKEN(anon_sym_While);
      END_STATE();
    case 243:
      ACCEPT_TOKEN(anon_sym_While);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 244:
      ACCEPT_TOKEN(anon_sym_If);
      END_STATE();
    case 245:
      ACCEPT_TOKEN(anon_sym_If);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 246:
      ACCEPT_TOKEN(anon_sym_Then);
      END_STATE();
    case 247:
      ACCEPT_TOKEN(anon_sym_Else);
      END_STATE();
    case 248:
      ACCEPT_TOKEN(anon_sym_Else);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 249:
      ACCEPT_TOKEN(anon_sym_EndIf);
      END_STATE();
    case 250:
      ACCEPT_TOKEN(anon_sym_To);
      END_STATE();
    case 251:
      ACCEPT_TOKEN(anon_sym_Step);
      END_STATE();
    case 252:
      ACCEPT_TOKEN(anon_sym_Next);
      END_STATE();
    case 253:
      ACCEPT_TOKEN(anon_sym_Next);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 254:
      ACCEPT_TOKEN(anon_sym_Wend);
      END_STATE();
    case 255:
      ACCEPT_TOKEN(anon_sym_Wend);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 256:
      ACCEPT_TOKEN(anon_sym_Loop);
      END_STATE();
    case 257:
      ACCEPT_TOKEN(anon_sym_Loop);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 258:
      ACCEPT_TOKEN(anon_sym_Until);
      END_STATE();
    case 259:
      ACCEPT_TOKEN(anon_sym_LPAREN);
      END_STATE();
    case 260:
      ACCEPT_TOKEN(anon_sym_RPAREN);
      END_STATE();
    case 261:
      ACCEPT_TOKEN(anon_sym_EndSub);
      END_STATE();
    case 262:
      ACCEPT_TOKEN(anon_sym_Private);
      END_STATE();
    case 263:
      ACCEPT_TOKEN(anon_sym_Private);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 264:
      ACCEPT_TOKEN(anon_sym_EndFunction);
      END_STATE();
    case 265:
      ACCEPT_TOKEN(anon_sym_Dim);
      END_STATE();
    case 266:
      ACCEPT_TOKEN(anon_sym_Dim);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 267:
      ACCEPT_TOKEN(anon_sym_ReDim);
      END_STATE();
    case 268:
      ACCEPT_TOKEN(anon_sym_ReDim);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 269:
      ACCEPT_TOKEN(anon_sym_Preserve);
      END_STATE();
    case 270:
      ACCEPT_TOKEN(anon_sym_Preserve);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 271:
      ACCEPT_TOKEN(anon_sym_As);
      END_STATE();
    case 272:
      ACCEPT_TOKEN(anon_sym_COMMA);
      END_STATE();
    case 273:
      ACCEPT_TOKEN(anon_sym_Declare);
      END_STATE();
    case 274:
      ACCEPT_TOKEN(anon_sym_PtrSafe);
      END_STATE();
    case 275:
      ACCEPT_TOKEN(anon_sym_Lib);
      END_STATE();
    case 276:
      ACCEPT_TOKEN(anon_sym_Alias);
      END_STATE();
    case 277:
      ACCEPT_TOKEN(anon_sym_ByVal);
      END_STATE();
    case 278:
      ACCEPT_TOKEN(anon_sym_ByVal);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 279:
      ACCEPT_TOKEN(anon_sym_ByRef);
      END_STATE();
    case 280:
      ACCEPT_TOKEN(anon_sym_ByRef);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 281:
      ACCEPT_TOKEN(anon_sym_Optional);
      END_STATE();
    case 282:
      ACCEPT_TOKEN(anon_sym_Optional);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 283:
      ACCEPT_TOKEN(anon_sym_ParamArray);
      END_STATE();
    case 284:
      ACCEPT_TOKEN(anon_sym_ParamArray);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 285:
      ACCEPT_TOKEN(anon_sym_Any);
      END_STATE();
    case 286:
      ACCEPT_TOKEN(anon_sym_Any);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 287:
      ACCEPT_TOKEN(anon_sym_Boolean);
      END_STATE();
    case 288:
      ACCEPT_TOKEN(anon_sym_Boolean);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 289:
      ACCEPT_TOKEN(anon_sym_Byte);
      END_STATE();
    case 290:
      ACCEPT_TOKEN(anon_sym_Byte);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 291:
      ACCEPT_TOKEN(anon_sym_Collection);
      END_STATE();
    case 292:
      ACCEPT_TOKEN(anon_sym_Collection);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 293:
      ACCEPT_TOKEN(anon_sym_Currency);
      END_STATE();
    case 294:
      ACCEPT_TOKEN(anon_sym_Currency);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 295:
      ACCEPT_TOKEN(anon_sym_Date);
      END_STATE();
    case 296:
      ACCEPT_TOKEN(anon_sym_Date);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 297:
      ACCEPT_TOKEN(anon_sym_Decimal);
      END_STATE();
    case 298:
      ACCEPT_TOKEN(anon_sym_Decimal);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 299:
      ACCEPT_TOKEN(anon_sym_Dictionary);
      END_STATE();
    case 300:
      ACCEPT_TOKEN(anon_sym_Dictionary);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 301:
      ACCEPT_TOKEN(anon_sym_Double);
      END_STATE();
    case 302:
      ACCEPT_TOKEN(anon_sym_Double);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 303:
      ACCEPT_TOKEN(anon_sym_Integer);
      END_STATE();
    case 304:
      ACCEPT_TOKEN(anon_sym_Integer);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 305:
      ACCEPT_TOKEN(anon_sym_Long);
      if (lookahead == 'L') ADVANCE(174);
      if (lookahead == 'P') ADVANCE(207);
      END_STATE();
    case 306:
      ACCEPT_TOKEN(anon_sym_Long);
      if (lookahead == 'L') ADVANCE(509);
      if (lookahead == 'P') ADVANCE(542);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 307:
      ACCEPT_TOKEN(anon_sym_LongLong);
      END_STATE();
    case 308:
      ACCEPT_TOKEN(anon_sym_LongLong);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 309:
      ACCEPT_TOKEN(anon_sym_LongPtr);
      END_STATE();
    case 310:
      ACCEPT_TOKEN(anon_sym_LongPtr);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 311:
      ACCEPT_TOKEN(anon_sym_Object);
      END_STATE();
    case 312:
      ACCEPT_TOKEN(anon_sym_Object);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 313:
      ACCEPT_TOKEN(anon_sym_Single);
      END_STATE();
    case 314:
      ACCEPT_TOKEN(anon_sym_Single);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 315:
      ACCEPT_TOKEN(anon_sym_String);
      END_STATE();
    case 316:
      ACCEPT_TOKEN(anon_sym_String);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 317:
      ACCEPT_TOKEN(anon_sym_Variant);
      END_STATE();
    case 318:
      ACCEPT_TOKEN(anon_sym_Variant);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 319:
      ACCEPT_TOKEN(anon_sym_LPAREN_RPAREN);
      END_STATE();
    case 320:
      ACCEPT_TOKEN(anon_sym_New);
      END_STATE();
    case 321:
      ACCEPT_TOKEN(anon_sym_New);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 322:
      ACCEPT_TOKEN(anon_sym_DOT);
      END_STATE();
    case 323:
      ACCEPT_TOKEN(anon_sym_PLUS);
      END_STATE();
    case 324:
      ACCEPT_TOKEN(anon_sym_DASH);
      END_STATE();
    case 325:
      ACCEPT_TOKEN(anon_sym_STAR);
      END_STATE();
    case 326:
      ACCEPT_TOKEN(anon_sym_SLASH);
      END_STATE();
    case 327:
      ACCEPT_TOKEN(anon_sym_Mod);
      END_STATE();
    case 328:
      ACCEPT_TOKEN(anon_sym_Mod);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 329:
      ACCEPT_TOKEN(anon_sym_AMP);
      END_STATE();
    case 330:
      ACCEPT_TOKEN(aux_sym_binary_expression_token1);
      END_STATE();
    case 331:
      ACCEPT_TOKEN(aux_sym_binary_expression_token1);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 332:
      ACCEPT_TOKEN(aux_sym_binary_expression_token2);
      END_STATE();
    case 333:
      ACCEPT_TOKEN(aux_sym_binary_expression_token2);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 334:
      ACCEPT_TOKEN(aux_sym_binary_expression_token3);
      END_STATE();
    case 335:
      ACCEPT_TOKEN(aux_sym_binary_expression_token3);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 336:
      ACCEPT_TOKEN(anon_sym_LT_GT);
      END_STATE();
    case 337:
      ACCEPT_TOKEN(anon_sym_LT);
      if (lookahead == '=') ADVANCE(339);
      if (lookahead == '>') ADVANCE(336);
      END_STATE();
    case 338:
      ACCEPT_TOKEN(anon_sym_GT);
      if (lookahead == '=') ADVANCE(340);
      END_STATE();
    case 339:
      ACCEPT_TOKEN(anon_sym_LT_EQ);
      END_STATE();
    case 340:
      ACCEPT_TOKEN(anon_sym_GT_EQ);
      END_STATE();
    case 341:
      ACCEPT_TOKEN(anon_sym_Not);
      END_STATE();
    case 342:
      ACCEPT_TOKEN(anon_sym_Not);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 343:
      ACCEPT_TOKEN(anon_sym_Call);
      END_STATE();
    case 344:
      ACCEPT_TOKEN(anon_sym_Call);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 345:
      ACCEPT_TOKEN(anon_sym_COLON_EQ);
      END_STATE();
    case 346:
      ACCEPT_TOKEN(sym_number);
      if (('0' <= lookahead && lookahead <= '9')) ADVANCE(346);
      END_STATE();
    case 347:
      ACCEPT_TOKEN(sym_number);
      if (('0' <= lookahead && lookahead <= '9')) ADVANCE(347);
      if (('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 348:
      ACCEPT_TOKEN(anon_sym_DQUOTE);
      END_STATE();
    case 349:
      ACCEPT_TOKEN(aux_sym_string_literal_token1);
      if (lookahead == '\'') ADVANCE(229);
      if (lookahead == '_') ADVANCE(349);
      if (lookahead == '\t' ||
          lookahead == ' ') ADVANCE(351);
      if (lookahead == '\n' ||
          lookahead == '\r') ADVANCE(349);
      if (lookahead != 0 &&
          lookahead != '"') ADVANCE(352);
      END_STATE();
    case 350:
      ACCEPT_TOKEN(aux_sym_string_literal_token1);
      if (lookahead == '\'') ADVANCE(229);
      if (lookahead == '_') ADVANCE(349);
      if (lookahead == '\t' ||
          lookahead == ' ') ADVANCE(351);
      if (lookahead != 0 &&
          lookahead != '"') ADVANCE(352);
      END_STATE();
    case 351:
      ACCEPT_TOKEN(aux_sym_string_literal_token1);
      if (lookahead == '\t' ||
          lookahead == ' ') ADVANCE(351);
      if (lookahead != 0 &&
          lookahead != '"') ADVANCE(352);
      END_STATE();
    case 352:
      ACCEPT_TOKEN(aux_sym_string_literal_token1);
      if (lookahead != 0 &&
          lookahead != '"') ADVANCE(352);
      END_STATE();
    case 353:
      ACCEPT_TOKEN(anon_sym_True);
      END_STATE();
    case 354:
      ACCEPT_TOKEN(anon_sym_True);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 355:
      ACCEPT_TOKEN(anon_sym_False);
      END_STATE();
    case 356:
      ACCEPT_TOKEN(anon_sym_False);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 357:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == ' ') ADVANCE(36);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 358:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == ' ') ADVANCE(34);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 359:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == ' ') ADVANCE(38);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 360:
      ACCEPT_TOKEN(sym_identifier);
      ADVANCE_MAP(
        'A', 483,
        'B', 502,
        'C', 387,
        'D', 403,
        'F', 391,
        'I', 498,
        'L', 506,
        'N', 419,
        'O', 405,
        'S', 458,
        'T', 519,
        'V', 402,
        '_', 360,
      );
      if (('0' <= lookahead && lookahead <= '9')) ADVANCE(347);
      if (('E' <= lookahead && lookahead <= 'Z') ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 361:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'A') ADVANCE(529);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('B' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 362:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'B') ADVANCE(559);
      if (lookahead == 'O') ADVANCE(513);
      if (lookahead == 'P') ADVANCE(400);
      if (lookahead == '_') ADVANCE(362);
      if (('0' <= lookahead && lookahead <= '9')) ADVANCE(561);
      if (('A' <= lookahead && lookahead <= 'Z') ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 363:
      ACCEPT_TOKEN(sym_identifier);
      ADVANCE_MAP(
        'C', 388,
        'D', 450,
        'E', 555,
        'F', 389,
        'I', 442,
        'N', 419,
        'P', 520,
        'R', 420,
        'S', 548,
        'T', 519,
        'W', 449,
        '_', 363,
      );
      if (('0' <= lookahead && lookahead <= '9')) ADVANCE(347);
      if (('A' <= lookahead && lookahead <= 'Z') ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 364:
      ACCEPT_TOKEN(sym_identifier);
      ADVANCE_MAP(
        'C', 388,
        'D', 450,
        'E', 555,
        'F', 389,
        'I', 442,
        'N', 419,
        'P', 520,
        'R', 420,
        'S', 548,
        'T', 519,
        'W', 449,
        '_', 364,
        '\n', 566,
        '\r', 566,
      );
      if (('0' <= lookahead && lookahead <= '9')) ADVANCE(347);
      if (('A' <= lookahead && lookahead <= 'Z') ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 365:
      ACCEPT_TOKEN(sym_identifier);
      ADVANCE_MAP(
        'C', 388,
        'D', 450,
        'E', 555,
        'F', 390,
        'I', 442,
        'L', 504,
        'N', 419,
        'R', 420,
        'T', 519,
        'W', 449,
        '_', 365,
      );
      if (('0' <= lookahead && lookahead <= '9')) ADVANCE(347);
      if (('A' <= lookahead && lookahead <= 'Z') ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 366:
      ACCEPT_TOKEN(sym_identifier);
      ADVANCE_MAP(
        'C', 388,
        'D', 450,
        'E', 555,
        'F', 390,
        'I', 442,
        'L', 504,
        'N', 419,
        'R', 420,
        'T', 519,
        'W', 449,
        '_', 366,
        '\n', 567,
        '\r', 567,
      );
      if (('0' <= lookahead && lookahead <= '9')) ADVANCE(347);
      if (('A' <= lookahead && lookahead <= 'Z') ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 367:
      ACCEPT_TOKEN(sym_identifier);
      ADVANCE_MAP(
        'C', 388,
        'D', 450,
        'E', 555,
        'F', 390,
        'I', 442,
        'N', 419,
        'R', 420,
        'T', 519,
        'W', 449,
        '_', 367,
        '\n', 564,
        '\r', 564,
      );
      if (('0' <= lookahead && lookahead <= '9')) ADVANCE(347);
      if (('A' <= lookahead && lookahead <= 'Z') ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 368:
      ACCEPT_TOKEN(sym_identifier);
      ADVANCE_MAP(
        'C', 388,
        'D', 450,
        'E', 555,
        'F', 390,
        'I', 442,
        'N', 419,
        'R', 420,
        'T', 519,
        'W', 436,
        '_', 368,
      );
      if (('0' <= lookahead && lookahead <= '9')) ADVANCE(347);
      if (('A' <= lookahead && lookahead <= 'Z') ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 369:
      ACCEPT_TOKEN(sym_identifier);
      ADVANCE_MAP(
        'C', 388,
        'D', 450,
        'E', 555,
        'F', 390,
        'I', 442,
        'N', 419,
        'R', 420,
        'T', 519,
        'W', 436,
        '_', 369,
        '\n', 570,
        '\r', 570,
      );
      if (('0' <= lookahead && lookahead <= '9')) ADVANCE(347);
      if (('A' <= lookahead && lookahead <= 'Z') ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 370:
      ACCEPT_TOKEN(sym_identifier);
      ADVANCE_MAP(
        'C', 388,
        'D', 450,
        'E', 555,
        'F', 390,
        'I', 442,
        'N', 430,
        'R', 420,
        'T', 519,
        'W', 449,
        '_', 370,
      );
      if (('0' <= lookahead && lookahead <= '9')) ADVANCE(347);
      if (('A' <= lookahead && lookahead <= 'Z') ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 371:
      ACCEPT_TOKEN(sym_identifier);
      ADVANCE_MAP(
        'C', 388,
        'D', 450,
        'E', 555,
        'F', 390,
        'I', 442,
        'N', 430,
        'R', 420,
        'T', 519,
        'W', 449,
        '_', 371,
        '\n', 571,
        '\r', 571,
      );
      if (('0' <= lookahead && lookahead <= '9')) ADVANCE(347);
      if (('A' <= lookahead && lookahead <= 'Z') ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 372:
      ACCEPT_TOKEN(sym_identifier);
      ADVANCE_MAP(
        'C', 388,
        'D', 450,
        'E', 474,
        'F', 390,
        'I', 442,
        'N', 419,
        'R', 420,
        'T', 519,
        'W', 449,
        '_', 372,
      );
      if (('0' <= lookahead && lookahead <= '9')) ADVANCE(347);
      if (('A' <= lookahead && lookahead <= 'Z') ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 373:
      ACCEPT_TOKEN(sym_identifier);
      ADVANCE_MAP(
        'C', 388,
        'D', 450,
        'E', 474,
        'F', 390,
        'I', 442,
        'N', 419,
        'R', 420,
        'T', 519,
        'W', 449,
        '_', 373,
        '\n', 568,
        '\r', 568,
      );
      if (('0' <= lookahead && lookahead <= '9')) ADVANCE(347);
      if (('A' <= lookahead && lookahead <= 'Z') ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 374:
      ACCEPT_TOKEN(sym_identifier);
      ADVANCE_MAP(
        'C', 388,
        'D', 450,
        'E', 496,
        'F', 390,
        'I', 442,
        'N', 419,
        'R', 420,
        'T', 519,
        'W', 449,
        '_', 374,
      );
      if (('0' <= lookahead && lookahead <= '9')) ADVANCE(347);
      if (('A' <= lookahead && lookahead <= 'Z') ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 375:
      ACCEPT_TOKEN(sym_identifier);
      ADVANCE_MAP(
        'C', 388,
        'D', 450,
        'E', 496,
        'F', 390,
        'I', 442,
        'N', 419,
        'R', 420,
        'T', 519,
        'W', 449,
        '_', 375,
        '\n', 573,
        '\r', 573,
      );
      if (('0' <= lookahead && lookahead <= '9')) ADVANCE(347);
      if (('A' <= lookahead && lookahead <= 'Z') ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 376:
      ACCEPT_TOKEN(sym_identifier);
      ADVANCE_MAP(
        'C', 388,
        'D', 450,
        'E', 487,
        'F', 390,
        'I', 442,
        'N', 419,
        'R', 420,
        'T', 519,
        'W', 449,
        '_', 376,
        '\n', 569,
        '\r', 569,
      );
      if (('0' <= lookahead && lookahead <= '9')) ADVANCE(347);
      if (('A' <= lookahead && lookahead <= 'Z') ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 377:
      ACCEPT_TOKEN(sym_identifier);
      ADVANCE_MAP(
        'C', 388,
        'D', 450,
        'E', 487,
        'F', 390,
        'I', 442,
        'N', 419,
        'R', 420,
        'T', 519,
        'W', 449,
        '_', 377,
      );
      if (('0' <= lookahead && lookahead <= '9')) ADVANCE(347);
      if (('A' <= lookahead && lookahead <= 'Z') ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 378:
      ACCEPT_TOKEN(sym_identifier);
      ADVANCE_MAP(
        'C', 388,
        'D', 450,
        'E', 497,
        'F', 390,
        'I', 442,
        'N', 419,
        'R', 420,
        'T', 519,
        'W', 449,
        '_', 378,
        '\n', 572,
        '\r', 572,
      );
      if (('0' <= lookahead && lookahead <= '9')) ADVANCE(347);
      if (('A' <= lookahead && lookahead <= 'Z') ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 379:
      ACCEPT_TOKEN(sym_identifier);
      ADVANCE_MAP(
        'C', 388,
        'D', 450,
        'E', 497,
        'F', 390,
        'I', 442,
        'N', 419,
        'R', 420,
        'T', 519,
        'W', 449,
        '_', 379,
      );
      if (('0' <= lookahead && lookahead <= '9')) ADVANCE(347);
      if (('A' <= lookahead && lookahead <= 'Z') ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 380:
      ACCEPT_TOKEN(sym_identifier);
      ADVANCE_MAP(
        'C', 388,
        'F', 391,
        'M', 503,
        'N', 419,
        'T', 519,
        '_', 380,
        '\n', 565,
        '\r', 565,
        'A', 492,
        'a', 492,
        'O', 517,
        'o', 517,
        'X', 508,
        'x', 508,
      );
      if (('0' <= lookahead && lookahead <= '9')) ADVANCE(347);
      if (('B' <= lookahead && lookahead <= 'Z') ||
          ('b' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 381:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'C') ADVANCE(388);
      if (lookahead == 'F') ADVANCE(391);
      if (lookahead == 'N') ADVANCE(419);
      if (lookahead == 'P') ADVANCE(530);
      if (lookahead == 'T') ADVANCE(519);
      if (lookahead == '_') ADVANCE(381);
      if (('0' <= lookahead && lookahead <= '9')) ADVANCE(347);
      if (('A' <= lookahead && lookahead <= 'Z') ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 382:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'C') ADVANCE(388);
      if (lookahead == 'F') ADVANCE(391);
      if (lookahead == 'N') ADVANCE(419);
      if (lookahead == 'T') ADVANCE(519);
      if (lookahead == '_') ADVANCE(382);
      if (('0' <= lookahead && lookahead <= '9')) ADVANCE(347);
      if (('A' <= lookahead && lookahead <= 'Z') ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 383:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'C') ADVANCE(388);
      if (lookahead == '_') ADVANCE(383);
      if (('0' <= lookahead && lookahead <= '9')) ADVANCE(561);
      if (('A' <= lookahead && lookahead <= 'Z') ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 384:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'D') ADVANCE(453);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 385:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'R') ADVANCE(432);
      if (lookahead == 'V') ADVANCE(396);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 386:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == '_') ADVANCE(386);
      if (('0' <= lookahead && lookahead <= '9')) ADVANCE(561);
      if (('A' <= lookahead && lookahead <= 'Z') ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 387:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'a') ADVANCE(469);
      if (lookahead == 'o') ADVANCE(475);
      if (lookahead == 'u') ADVANCE(523);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('b' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 388:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'a') ADVANCE(469);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('b' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 389:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'a') ADVANCE(464);
      if (lookahead == 'o') ADVANCE(514);
      if (lookahead == 'u') ADVANCE(481);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('b' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 390:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'a') ADVANCE(464);
      if (lookahead == 'o') ADVANCE(514);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('b' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 391:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'a') ADVANCE(464);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('b' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 392:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'a') ADVANCE(479);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('b' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 393:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'a') ADVANCE(539);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('b' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 394:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'a') ADVANCE(560);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('b' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 395:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'a') ADVANCE(466);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('b' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 396:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'a') ADVANCE(467);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('b' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 397:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'a') ADVANCE(522);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('b' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 398:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'a') ADVANCE(468);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('b' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 399:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'a') ADVANCE(485);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('b' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 400:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'a') ADVANCE(524);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('b' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 401:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'a') ADVANCE(495);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('b' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 402:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'a') ADVANCE(527);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('b' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 403:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'a') ADVANCE(543);
      if (lookahead == 'e') ADVANCE(409);
      if (lookahead == 'i') ADVANCE(411);
      if (lookahead == 'o') ADVANCE(550);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('b' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 404:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'b') ADVANCE(238);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 405:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'b') ADVANCE(463);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 406:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'b') ADVANCE(472);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 407:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'c') ADVANCE(557);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 408:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'c') ADVANCE(540);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 409:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'c') ADVANCE(455);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 410:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'c') ADVANCE(536);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 411:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'c') ADVANCE(545);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 412:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'c') ADVANCE(546);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 413:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'd') ADVANCE(357);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 414:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'd') ADVANCE(255);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 415:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'd') ADVANCE(328);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 416:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'd') ADVANCE(331);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 417:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'd') ADVANCE(358);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 418:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'd') ADVANCE(359);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 419:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'e') ADVANCE(554);
      if (lookahead == 'o') ADVANCE(534);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 420:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'e') ADVANCE(384);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 421:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'e') ADVANCE(354);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 422:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'e') ADVANCE(356);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 423:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'e') ADVANCE(243);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 424:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'e') ADVANCE(263);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 425:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'e') ADVANCE(290);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 426:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'e') ADVANCE(296);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 427:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'e') ADVANCE(302);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 428:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'e') ADVANCE(314);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 429:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'e') ADVANCE(248);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 430:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'e') ADVANCE(553);
      if (lookahead == 'o') ADVANCE(534);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 431:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'e') ADVANCE(270);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 432:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'e') ADVANCE(443);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 433:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'e') ADVANCE(447);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 434:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'e') ADVANCE(515);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 435:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'e') ADVANCE(521);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 436:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'e') ADVANCE(489);
      if (lookahead == 'h') ADVANCE(457);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 437:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'e') ADVANCE(490);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 438:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'e') ADVANCE(410);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 439:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'e') ADVANCE(399);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 440:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'e') ADVANCE(533);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 441:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'e') ADVANCE(412);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 442:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'f') ADVANCE(245);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 443:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'f') ADVANCE(280);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 444:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'g') ADVANCE(306);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 445:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'g') ADVANCE(316);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 446:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'g') ADVANCE(308);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 447:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'g') ADVANCE(434);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 448:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'g') ADVANCE(473);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 449:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'h') ADVANCE(457);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 450:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'i') ADVANCE(477);
      if (lookahead == 'o') ADVANCE(241);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 451:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'i') ADVANCE(551);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 452:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'i') ADVANCE(501);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 453:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'i') ADVANCE(478);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 454:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'i') ADVANCE(535);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 455:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'i') ADVANCE(480);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 456:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'i') ADVANCE(401);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 457:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'i') ADVANCE(470);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 458:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'i') ADVANCE(488);
      if (lookahead == 't') ADVANCE(526);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 459:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'i') ADVANCE(491);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 460:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'i') ADVANCE(507);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 461:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'i') ADVANCE(510);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 462:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'i') ADVANCE(511);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 463:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'j') ADVANCE(438);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 464:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'l') ADVANCE(531);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 465:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'l') ADVANCE(344);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 466:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'l') ADVANCE(298);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 467:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'l') ADVANCE(278);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 468:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'l') ADVANCE(282);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 469:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'l') ADVANCE(465);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 470:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'l') ADVANCE(423);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 471:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'l') ADVANCE(439);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 472:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'l') ADVANCE(427);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 473:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'l') ADVANCE(428);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 474:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'l') ADVANCE(532);
      if (lookahead == 'n') ADVANCE(413);
      if (lookahead == 'x') ADVANCE(454);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 475:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'l') ADVANCE(476);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 476:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'l') ADVANCE(441);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 477:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'm') ADVANCE(266);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 478:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'm') ADVANCE(268);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 479:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'm') ADVANCE(361);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 480:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'm') ADVANCE(395);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 481:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'n') ADVANCE(408);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 482:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'n') ADVANCE(236);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 483:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'n') ADVANCE(556);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 484:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'n') ADVANCE(444);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 485:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'n') ADVANCE(288);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 486:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'n') ADVANCE(292);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 487:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'n') ADVANCE(413);
      if (lookahead == 'x') ADVANCE(454);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 488:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'n') ADVANCE(448);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 489:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'n') ADVANCE(414);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 490:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'n') ADVANCE(407);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 491:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'n') ADVANCE(445);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 492:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'n') ADVANCE(416);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 493:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'n') ADVANCE(397);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 494:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'n') ADVANCE(446);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 495:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'n') ADVANCE(537);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 496:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'n') ADVANCE(417);
      if (lookahead == 'x') ADVANCE(454);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 497:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'n') ADVANCE(418);
      if (lookahead == 'x') ADVANCE(454);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 498:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'n') ADVANCE(544);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 499:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'n') ADVANCE(398);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 500:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'o') ADVANCE(512);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 501:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'o') ADVANCE(482);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 502:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'o') ADVANCE(505);
      if (lookahead == 'y') ADVANCE(541);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 503:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'o') ADVANCE(415);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 504:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'o') ADVANCE(500);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 505:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'o') ADVANCE(471);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 506:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'o') ADVANCE(484);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 507:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'o') ADVANCE(493);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 508:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'o') ADVANCE(518);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 509:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'o') ADVANCE(494);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 510:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'o') ADVANCE(486);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 511:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'o') ADVANCE(499);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 512:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'p') ADVANCE(257);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 513:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'p') ADVANCE(547);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 514:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'r') ADVANCE(234);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 515:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'r') ADVANCE(304);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 516:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'r') ADVANCE(310);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 517:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'r') ADVANCE(333);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 518:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'r') ADVANCE(335);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 519:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'r') ADVANCE(549);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 520:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'r') ADVANCE(451);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 521:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'r') ADVANCE(552);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 522:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'r') ADVANCE(558);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 523:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'r') ADVANCE(528);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 524:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'r') ADVANCE(392);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 525:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'r') ADVANCE(394);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 526:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'r') ADVANCE(459);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 527:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'r') ADVANCE(456);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 528:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'r') ADVANCE(437);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 529:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'r') ADVANCE(525);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 530:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'r') ADVANCE(440);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 531:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 's') ADVANCE(422);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 532:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 's') ADVANCE(429);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 533:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 's') ADVANCE(435);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 534:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 't') ADVANCE(342);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 535:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 't') ADVANCE(232);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 536:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 't') ADVANCE(312);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 537:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 't') ADVANCE(318);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 538:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 't') ADVANCE(253);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 539:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 't') ADVANCE(424);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 540:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 't') ADVANCE(452);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 541:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 't') ADVANCE(425);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 542:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 't') ADVANCE(516);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 543:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 't') ADVANCE(426);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 544:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 't') ADVANCE(433);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 545:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 't') ADVANCE(460);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 546:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 't') ADVANCE(461);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 547:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 't') ADVANCE(462);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 548:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'u') ADVANCE(404);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 549:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'u') ADVANCE(421);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 550:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'u') ADVANCE(406);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 551:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'v') ADVANCE(393);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 552:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'v') ADVANCE(431);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 553:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'w') ADVANCE(321);
      if (lookahead == 'x') ADVANCE(538);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 554:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'w') ADVANCE(321);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 555:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'x') ADVANCE(454);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 556:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'y') ADVANCE(286);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 557:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'y') ADVANCE(294);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 558:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'y') ADVANCE(300);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 559:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'y') ADVANCE(385);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 560:
      ACCEPT_TOKEN(sym_identifier);
      if (lookahead == 'y') ADVANCE(284);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 561:
      ACCEPT_TOKEN(sym_identifier);
      if (('0' <= lookahead && lookahead <= '9') ||
          ('A' <= lookahead && lookahead <= 'Z') ||
          lookahead == '_' ||
          ('a' <= lookahead && lookahead <= 'z')) ADVANCE(561);
      END_STATE();
    case 562:
      ACCEPT_TOKEN(sym__equal);
      END_STATE();
    case 563:
      ACCEPT_TOKEN(aux_sym__whitespace_token1);
      END_STATE();
    case 564:
      ACCEPT_TOKEN(aux_sym__whitespace_token1);
      if (lookahead == '_') ADVANCE(367);
      if (lookahead == '\n' ||
          lookahead == '\r') ADVANCE(564);
      END_STATE();
    case 565:
      ACCEPT_TOKEN(aux_sym__whitespace_token1);
      if (lookahead == '_') ADVANCE(380);
      if (lookahead == '\n' ||
          lookahead == '\r') ADVANCE(565);
      END_STATE();
    case 566:
      ACCEPT_TOKEN(aux_sym__whitespace_token1);
      if (lookahead == '_') ADVANCE(364);
      if (lookahead == '\n' ||
          lookahead == '\r') ADVANCE(566);
      END_STATE();
    case 567:
      ACCEPT_TOKEN(aux_sym__whitespace_token1);
      if (lookahead == '_') ADVANCE(366);
      if (lookahead == '\n' ||
          lookahead == '\r') ADVANCE(567);
      END_STATE();
    case 568:
      ACCEPT_TOKEN(aux_sym__whitespace_token1);
      if (lookahead == '_') ADVANCE(373);
      if (lookahead == '\n' ||
          lookahead == '\r') ADVANCE(568);
      END_STATE();
    case 569:
      ACCEPT_TOKEN(aux_sym__whitespace_token1);
      if (lookahead == '_') ADVANCE(376);
      if (lookahead == '\n' ||
          lookahead == '\r') ADVANCE(569);
      END_STATE();
    case 570:
      ACCEPT_TOKEN(aux_sym__whitespace_token1);
      if (lookahead == '_') ADVANCE(369);
      if (lookahead == '\n' ||
          lookahead == '\r') ADVANCE(570);
      END_STATE();
    case 571:
      ACCEPT_TOKEN(aux_sym__whitespace_token1);
      if (lookahead == '_') ADVANCE(371);
      if (lookahead == '\n' ||
          lookahead == '\r') ADVANCE(571);
      END_STATE();
    case 572:
      ACCEPT_TOKEN(aux_sym__whitespace_token1);
      if (lookahead == '_') ADVANCE(378);
      if (lookahead == '\n' ||
          lookahead == '\r') ADVANCE(572);
      END_STATE();
    case 573:
      ACCEPT_TOKEN(aux_sym__whitespace_token1);
      if (lookahead == '_') ADVANCE(375);
      if (lookahead == '\n' ||
          lookahead == '\r') ADVANCE(573);
      END_STATE();
    case 574:
      ACCEPT_TOKEN(aux_sym__whitespace_token1);
      if (lookahead == '\n' ||
          lookahead == '\r') ADVANCE(574);
      END_STATE();
    case 575:
      ACCEPT_TOKEN(sym__horizontal_whitespace);
      if (lookahead == '\t' ||
          lookahead == ' ') ADVANCE(575);
      END_STATE();
    default:
      return false;
  }
}

static const TSLexMode ts_lex_modes[STATE_COUNT] = {
  [0] = {.lex_state = 0},
  [1] = {.lex_state = 226},
  [2] = {.lex_state = 226},
  [3] = {.lex_state = 226},
  [4] = {.lex_state = 7},
  [5] = {.lex_state = 7},
  [6] = {.lex_state = 7},
  [7] = {.lex_state = 7},
  [8] = {.lex_state = 7},
  [9] = {.lex_state = 7},
  [10] = {.lex_state = 7},
  [11] = {.lex_state = 7},
  [12] = {.lex_state = 7},
  [13] = {.lex_state = 7},
  [14] = {.lex_state = 7},
  [15] = {.lex_state = 7},
  [16] = {.lex_state = 4},
  [17] = {.lex_state = 4},
  [18] = {.lex_state = 12},
  [19] = {.lex_state = 12},
  [20] = {.lex_state = 5},
  [21] = {.lex_state = 10},
  [22] = {.lex_state = 8},
  [23] = {.lex_state = 14},
  [24] = {.lex_state = 17},
  [25] = {.lex_state = 8},
  [26] = {.lex_state = 17},
  [27] = {.lex_state = 14},
  [28] = {.lex_state = 19},
  [29] = {.lex_state = 5},
  [30] = {.lex_state = 10},
  [31] = {.lex_state = 19},
  [32] = {.lex_state = 2},
  [33] = {.lex_state = 2},
  [34] = {.lex_state = 2},
  [35] = {.lex_state = 2},
  [36] = {.lex_state = 2},
  [37] = {.lex_state = 2},
  [38] = {.lex_state = 2},
  [39] = {.lex_state = 2},
  [40] = {.lex_state = 2},
  [41] = {.lex_state = 2},
  [42] = {.lex_state = 2},
  [43] = {.lex_state = 2},
  [44] = {.lex_state = 2},
  [45] = {.lex_state = 2},
  [46] = {.lex_state = 2},
  [47] = {.lex_state = 2},
  [48] = {.lex_state = 2},
  [49] = {.lex_state = 2},
  [50] = {.lex_state = 2},
  [51] = {.lex_state = 2},
  [52] = {.lex_state = 3},
  [53] = {.lex_state = 3},
  [54] = {.lex_state = 227},
  [55] = {.lex_state = 3},
  [56] = {.lex_state = 3},
  [57] = {.lex_state = 3},
  [58] = {.lex_state = 227},
  [59] = {.lex_state = 3},
  [60] = {.lex_state = 3},
  [61] = {.lex_state = 0},
  [62] = {.lex_state = 0},
  [63] = {.lex_state = 3},
  [64] = {.lex_state = 0},
  [65] = {.lex_state = 13},
  [66] = {.lex_state = 13},
  [67] = {.lex_state = 3},
  [68] = {.lex_state = 0},
  [69] = {.lex_state = 13},
  [70] = {.lex_state = 24},
  [71] = {.lex_state = 16},
  [72] = {.lex_state = 9},
  [73] = {.lex_state = 16},
  [74] = {.lex_state = 9},
  [75] = {.lex_state = 16},
  [76] = {.lex_state = 11},
  [77] = {.lex_state = 3},
  [78] = {.lex_state = 0},
  [79] = {.lex_state = 0},
  [80] = {.lex_state = 18},
  [81] = {.lex_state = 0},
  [82] = {.lex_state = 0},
  [83] = {.lex_state = 0},
  [84] = {.lex_state = 6},
  [85] = {.lex_state = 0},
  [86] = {.lex_state = 0},
  [87] = {.lex_state = 0},
  [88] = {.lex_state = 0},
  [89] = {.lex_state = 11},
  [90] = {.lex_state = 21},
  [91] = {.lex_state = 3},
  [92] = {.lex_state = 0},
  [93] = {.lex_state = 25},
  [94] = {.lex_state = 3},
  [95] = {.lex_state = 20},
  [96] = {.lex_state = 9},
  [97] = {.lex_state = 11},
  [98] = {.lex_state = 15},
  [99] = {.lex_state = 0},
  [100] = {.lex_state = 21},
  [101] = {.lex_state = 0},
  [102] = {.lex_state = 6},
  [103] = {.lex_state = 21},
  [104] = {.lex_state = 18},
  [105] = {.lex_state = 15},
  [106] = {.lex_state = 15},
  [107] = {.lex_state = 21},
  [108] = {.lex_state = 0},
  [109] = {.lex_state = 18},
  [110] = {.lex_state = 6},
  [111] = {.lex_state = 3},
  [112] = {.lex_state = 3},
  [113] = {.lex_state = 21},
  [114] = {.lex_state = 21},
  [115] = {.lex_state = 3},
  [116] = {.lex_state = 3},
  [117] = {.lex_state = 3},
  [118] = {.lex_state = 3},
  [119] = {.lex_state = 21},
  [120] = {.lex_state = 21},
  [121] = {.lex_state = 3},
  [122] = {.lex_state = 3},
  [123] = {.lex_state = 3},
  [124] = {.lex_state = 3},
  [125] = {.lex_state = 3},
  [126] = {.lex_state = 3},
  [127] = {.lex_state = 21},
  [128] = {.lex_state = 3},
  [129] = {.lex_state = 3},
  [130] = {.lex_state = 3},
  [131] = {.lex_state = 21},
  [132] = {.lex_state = 21},
  [133] = {.lex_state = 21},
  [134] = {.lex_state = 3},
  [135] = {.lex_state = 3},
  [136] = {.lex_state = 3},
  [137] = {.lex_state = 3},
  [138] = {.lex_state = 21},
  [139] = {.lex_state = 21},
  [140] = {.lex_state = 21},
  [141] = {.lex_state = 3},
  [142] = {.lex_state = 3},
  [143] = {.lex_state = 21},
  [144] = {.lex_state = 3},
  [145] = {.lex_state = 3},
  [146] = {.lex_state = 3},
  [147] = {.lex_state = 21},
  [148] = {.lex_state = 3},
  [149] = {.lex_state = 3},
  [150] = {.lex_state = 3},
  [151] = {.lex_state = 21},
  [152] = {.lex_state = 3},
  [153] = {.lex_state = 0},
  [154] = {.lex_state = 21},
  [155] = {.lex_state = 0},
  [156] = {.lex_state = 21},
  [157] = {.lex_state = 0},
  [158] = {.lex_state = 21},
  [159] = {.lex_state = 21},
  [160] = {.lex_state = 21},
  [161] = {.lex_state = 21},
  [162] = {.lex_state = 0},
  [163] = {.lex_state = 0},
  [164] = {.lex_state = 21},
  [165] = {.lex_state = 0},
  [166] = {.lex_state = 21},
  [167] = {.lex_state = 21},
  [168] = {.lex_state = 21},
  [169] = {.lex_state = 0},
  [170] = {.lex_state = 0},
  [171] = {.lex_state = 0},
  [172] = {.lex_state = 0},
  [173] = {.lex_state = 0},
  [174] = {.lex_state = 0},
  [175] = {.lex_state = 0},
  [176] = {.lex_state = 0},
  [177] = {.lex_state = 27},
  [178] = {.lex_state = 27},
  [179] = {.lex_state = 27},
  [180] = {.lex_state = 27},
  [181] = {.lex_state = 27},
  [182] = {.lex_state = 27},
  [183] = {.lex_state = 29},
  [184] = {.lex_state = 21},
  [185] = {.lex_state = 0},
  [186] = {.lex_state = 21},
  [187] = {.lex_state = 24},
  [188] = {.lex_state = 21},
  [189] = {.lex_state = 21},
  [190] = {.lex_state = 21},
  [191] = {.lex_state = 21},
  [192] = {.lex_state = 21},
  [193] = {.lex_state = 21},
  [194] = {.lex_state = 21},
  [195] = {.lex_state = 29},
  [196] = {.lex_state = 21},
  [197] = {.lex_state = 0},
  [198] = {.lex_state = 0},
  [199] = {.lex_state = 21},
  [200] = {.lex_state = 25},
  [201] = {.lex_state = 0},
  [202] = {.lex_state = 28},
  [203] = {.lex_state = 21},
  [204] = {.lex_state = 24},
  [205] = {.lex_state = 0},
  [206] = {.lex_state = 28},
  [207] = {.lex_state = 0},
  [208] = {.lex_state = 0},
  [209] = {.lex_state = 21},
  [210] = {.lex_state = 24},
  [211] = {.lex_state = 21},
  [212] = {.lex_state = 24},
  [213] = {.lex_state = 24},
  [214] = {.lex_state = 21},
  [215] = {.lex_state = 25},
  [216] = {.lex_state = 0},
  [217] = {.lex_state = 25},
  [218] = {.lex_state = 25},
  [219] = {.lex_state = 28},
  [220] = {.lex_state = 0},
  [221] = {.lex_state = 0},
  [222] = {.lex_state = 0},
  [223] = {.lex_state = 0},
  [224] = {.lex_state = 0},
  [225] = {.lex_state = 29},
  [226] = {.lex_state = 29},
  [227] = {.lex_state = 0},
  [228] = {.lex_state = 21},
  [229] = {.lex_state = 0},
  [230] = {.lex_state = 21},
  [231] = {.lex_state = 21},
  [232] = {.lex_state = 0},
  [233] = {.lex_state = 21},
  [234] = {.lex_state = 21},
  [235] = {.lex_state = 21},
  [236] = {.lex_state = 21},
  [237] = {.lex_state = 29},
  [238] = {.lex_state = 21},
  [239] = {.lex_state = 0},
  [240] = {.lex_state = 21},
  [241] = {.lex_state = 21},
  [242] = {.lex_state = 21},
  [243] = {.lex_state = 21},
  [244] = {.lex_state = 0},
  [245] = {.lex_state = 29},
  [246] = {.lex_state = 21},
  [247] = {.lex_state = 0},
  [248] = {.lex_state = 0},
  [249] = {.lex_state = 29},
  [250] = {.lex_state = 0},
  [251] = {.lex_state = 0},
  [252] = {.lex_state = 21},
  [253] = {.lex_state = 0},
  [254] = {.lex_state = 21},
  [255] = {.lex_state = 21},
  [256] = {.lex_state = 21},
  [257] = {.lex_state = 0},
  [258] = {.lex_state = 0},
  [259] = {.lex_state = 21},
  [260] = {.lex_state = 21},
  [261] = {.lex_state = 0},
  [262] = {.lex_state = 0},
  [263] = {.lex_state = 0},
  [264] = {.lex_state = 21},
  [265] = {.lex_state = 21},
  [266] = {.lex_state = 0},
  [267] = {.lex_state = 0},
  [268] = {.lex_state = 21},
  [269] = {.lex_state = 0},
  [270] = {.lex_state = 0},
  [271] = {.lex_state = 0},
  [272] = {.lex_state = 0},
  [273] = {.lex_state = 0},
  [274] = {.lex_state = 21},
  [275] = {.lex_state = 29},
  [276] = {.lex_state = 0},
  [277] = {.lex_state = 21},
  [278] = {.lex_state = 21},
  [279] = {.lex_state = 21},
  [280] = {.lex_state = 0},
  [281] = {.lex_state = 21},
  [282] = {.lex_state = 0},
  [283] = {.lex_state = 0},
  [284] = {.lex_state = 21},
  [285] = {.lex_state = 0},
  [286] = {.lex_state = 29},
  [287] = {.lex_state = 0},
  [288] = {.lex_state = 29},
  [289] = {.lex_state = 0},
  [290] = {.lex_state = 0},
  [291] = {.lex_state = 0},
  [292] = {.lex_state = 29},
  [293] = {.lex_state = 0},
  [294] = {.lex_state = 21},
  [295] = {.lex_state = 21},
  [296] = {.lex_state = 21},
  [297] = {.lex_state = 0},
  [298] = {.lex_state = 0},
  [299] = {.lex_state = 21},
  [300] = {.lex_state = 0},
  [301] = {.lex_state = 0},
  [302] = {.lex_state = 0},
  [303] = {.lex_state = 0},
  [304] = {.lex_state = 0},
  [305] = {.lex_state = 29},
  [306] = {.lex_state = 0},
  [307] = {.lex_state = 0},
  [308] = {.lex_state = 0},
  [309] = {.lex_state = 0},
  [310] = {.lex_state = 21},
  [311] = {.lex_state = 0},
  [312] = {.lex_state = 0},
  [313] = {.lex_state = 0},
  [314] = {.lex_state = 0},
  [315] = {.lex_state = 0},
  [316] = {.lex_state = 29},
  [317] = {.lex_state = 350},
  [318] = {.lex_state = 0},
  [319] = {.lex_state = 0},
  [320] = {.lex_state = 21},
  [321] = {.lex_state = 21},
  [322] = {.lex_state = 21},
  [323] = {.lex_state = 29},
  [324] = {.lex_state = 29},
  [325] = {.lex_state = 350},
  [326] = {.lex_state = 0},
  [327] = {.lex_state = 0},
  [328] = {.lex_state = 21},
  [329] = {.lex_state = 0},
  [330] = {.lex_state = 350},
  [331] = {.lex_state = 21},
  [332] = {.lex_state = 0},
  [333] = {.lex_state = 29},
  [334] = {.lex_state = 29},
};

static const uint16_t ts_parse_table[LARGE_STATE_COUNT][SYMBOL_COUNT] = {
  [0] = {
    [ts_builtin_sym_end] = ACTIONS(1),
    [sym_comment] = ACTIONS(3),
    [anon_sym_Exit] = ACTIONS(1),
    [anon_sym_For] = ACTIONS(1),
    [anon_sym_Function] = ACTIONS(1),
    [anon_sym_Sub] = ACTIONS(1),
    [anon_sym_Do] = ACTIONS(1),
    [anon_sym_While] = ACTIONS(1),
    [anon_sym_If] = ACTIONS(1),
    [anon_sym_Then] = ACTIONS(1),
    [anon_sym_Else] = ACTIONS(1),
    [anon_sym_EndIf] = ACTIONS(1),
    [anon_sym_To] = ACTIONS(1),
    [anon_sym_Step] = ACTIONS(1),
    [anon_sym_Next] = ACTIONS(1),
    [anon_sym_Wend] = ACTIONS(1),
    [anon_sym_Loop] = ACTIONS(1),
    [anon_sym_Until] = ACTIONS(1),
    [anon_sym_LPAREN] = ACTIONS(1),
    [anon_sym_RPAREN] = ACTIONS(1),
    [anon_sym_EndSub] = ACTIONS(1),
    [anon_sym_Private] = ACTIONS(1),
    [anon_sym_EndFunction] = ACTIONS(1),
    [anon_sym_Dim] = ACTIONS(1),
    [anon_sym_ReDim] = ACTIONS(1),
    [anon_sym_Preserve] = ACTIONS(1),
    [anon_sym_As] = ACTIONS(1),
    [anon_sym_COMMA] = ACTIONS(1),
    [anon_sym_Declare] = ACTIONS(1),
    [anon_sym_PtrSafe] = ACTIONS(1),
    [anon_sym_Lib] = ACTIONS(1),
    [anon_sym_Alias] = ACTIONS(1),
    [anon_sym_ByVal] = ACTIONS(1),
    [anon_sym_ByRef] = ACTIONS(1),
    [anon_sym_Optional] = ACTIONS(1),
    [anon_sym_ParamArray] = ACTIONS(1),
    [anon_sym_Any] = ACTIONS(1),
    [anon_sym_Boolean] = ACTIONS(1),
    [anon_sym_Byte] = ACTIONS(1),
    [anon_sym_Collection] = ACTIONS(1),
    [anon_sym_Currency] = ACTIONS(1),
    [anon_sym_Date] = ACTIONS(1),
    [anon_sym_Decimal] = ACTIONS(1),
    [anon_sym_Dictionary] = ACTIONS(1),
    [anon_sym_Double] = ACTIONS(1),
    [anon_sym_Integer] = ACTIONS(1),
    [anon_sym_Long] = ACTIONS(1),
    [anon_sym_LongLong] = ACTIONS(1),
    [anon_sym_LongPtr] = ACTIONS(1),
    [anon_sym_Object] = ACTIONS(1),
    [anon_sym_Single] = ACTIONS(1),
    [anon_sym_String] = ACTIONS(1),
    [anon_sym_Variant] = ACTIONS(1),
    [anon_sym_New] = ACTIONS(1),
    [anon_sym_DOT] = ACTIONS(1),
    [anon_sym_PLUS] = ACTIONS(1),
    [anon_sym_DASH] = ACTIONS(1),
    [anon_sym_STAR] = ACTIONS(1),
    [anon_sym_SLASH] = ACTIONS(1),
    [anon_sym_Mod] = ACTIONS(1),
    [anon_sym_AMP] = ACTIONS(1),
    [aux_sym_binary_expression_token1] = ACTIONS(1),
    [aux_sym_binary_expression_token2] = ACTIONS(1),
    [aux_sym_binary_expression_token3] = ACTIONS(1),
    [anon_sym_LT_GT] = ACTIONS(1),
    [anon_sym_LT] = ACTIONS(1),
    [anon_sym_GT] = ACTIONS(1),
    [anon_sym_LT_EQ] = ACTIONS(1),
    [anon_sym_GT_EQ] = ACTIONS(1),
    [anon_sym_Not] = ACTIONS(1),
    [anon_sym_Call] = ACTIONS(1),
    [anon_sym_COLON_EQ] = ACTIONS(1),
    [sym_number] = ACTIONS(1),
    [anon_sym_DQUOTE] = ACTIONS(1),
    [anon_sym_True] = ACTIONS(1),
    [anon_sym_False] = ACTIONS(1),
    [sym__equal] = ACTIONS(1),
    [sym__horizontal_whitespace] = ACTIONS(3),
  },
  [1] = {
    [sym_source_file] = STATE(315),
    [sym__statement] = STATE(236),
    [sym__inline_statement] = STATE(236),
    [sym_variable_assignment] = STATE(236),
    [sym_invocation_statement] = STATE(236),
    [sym_exit_statement] = STATE(236),
    [sym_if_statement] = STATE(236),
    [sym_for_statement] = STATE(236),
    [sym_while_statement] = STATE(236),
    [sym_do_statement] = STATE(236),
    [sym_subroutine] = STATE(236),
    [sym_function] = STATE(236),
    [sym_variable_declaration] = STATE(236),
    [sym_redim] = STATE(236),
    [sym_ptrsafe_function_declaration] = STATE(236),
    [sym__expression] = STATE(32),
    [sym_new_expression] = STATE(32),
    [sym_member_expression] = STATE(32),
    [sym_binary_expression] = STATE(32),
    [sym_unary_expression] = STATE(32),
    [sym_function_call] = STATE(32),
    [sym_array_element] = STATE(311),
    [sym_literal] = STATE(32),
    [sym_string_literal] = STATE(46),
    [sym_boolean] = STATE(46),
    [aux_sym_source_file_repeat1] = STATE(3),
    [ts_builtin_sym_end] = ACTIONS(5),
    [sym_comment] = ACTIONS(7),
    [anon_sym_Exit] = ACTIONS(9),
    [anon_sym_For] = ACTIONS(11),
    [anon_sym_Function] = ACTIONS(13),
    [anon_sym_Sub] = ACTIONS(15),
    [anon_sym_Do] = ACTIONS(17),
    [anon_sym_While] = ACTIONS(19),
    [anon_sym_If] = ACTIONS(21),
    [anon_sym_LPAREN] = ACTIONS(23),
    [anon_sym_Private] = ACTIONS(25),
    [anon_sym_Dim] = ACTIONS(27),
    [anon_sym_ReDim] = ACTIONS(29),
    [anon_sym_New] = ACTIONS(31),
    [anon_sym_DASH] = ACTIONS(33),
    [anon_sym_Not] = ACTIONS(33),
    [anon_sym_Call] = ACTIONS(35),
    [sym_number] = ACTIONS(37),
    [anon_sym_DQUOTE] = ACTIONS(39),
    [anon_sym_True] = ACTIONS(41),
    [anon_sym_False] = ACTIONS(41),
    [sym_identifier] = ACTIONS(43),
    [sym__horizontal_whitespace] = ACTIONS(7),
  },
};

static const uint16_t ts_small_parse_table[] = {
  [0] = 25,
    ACTIONS(45), 1,
      ts_builtin_sym_end,
    ACTIONS(47), 1,
      anon_sym_Exit,
    ACTIONS(50), 1,
      anon_sym_For,
    ACTIONS(53), 1,
      anon_sym_Function,
    ACTIONS(56), 1,
      anon_sym_Sub,
    ACTIONS(59), 1,
      anon_sym_Do,
    ACTIONS(62), 1,
      anon_sym_While,
    ACTIONS(65), 1,
      anon_sym_If,
    ACTIONS(68), 1,
      anon_sym_LPAREN,
    ACTIONS(71), 1,
      anon_sym_Private,
    ACTIONS(74), 1,
      anon_sym_Dim,
    ACTIONS(77), 1,
      anon_sym_ReDim,
    ACTIONS(80), 1,
      anon_sym_New,
    ACTIONS(86), 1,
      anon_sym_Call,
    ACTIONS(89), 1,
      sym_number,
    ACTIONS(92), 1,
      anon_sym_DQUOTE,
    ACTIONS(98), 1,
      sym_identifier,
    STATE(2), 1,
      aux_sym_source_file_repeat1,
    STATE(311), 1,
      sym_array_element,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(83), 2,
      anon_sym_DASH,
      anon_sym_Not,
    ACTIONS(95), 2,
      anon_sym_True,
      anon_sym_False,
    STATE(46), 2,
      sym_string_literal,
      sym_boolean,
    STATE(32), 7,
      sym__expression,
      sym_new_expression,
      sym_member_expression,
      sym_binary_expression,
      sym_unary_expression,
      sym_function_call,
      sym_literal,
    STATE(236), 14,
      sym__statement,
      sym__inline_statement,
      sym_variable_assignment,
      sym_invocation_statement,
      sym_exit_statement,
      sym_if_statement,
      sym_for_statement,
      sym_while_statement,
      sym_do_statement,
      sym_subroutine,
      sym_function,
      sym_variable_declaration,
      sym_redim,
      sym_ptrsafe_function_declaration,
  [99] = 25,
    ACTIONS(9), 1,
      anon_sym_Exit,
    ACTIONS(11), 1,
      anon_sym_For,
    ACTIONS(13), 1,
      anon_sym_Function,
    ACTIONS(15), 1,
      anon_sym_Sub,
    ACTIONS(17), 1,
      anon_sym_Do,
    ACTIONS(19), 1,
      anon_sym_While,
    ACTIONS(21), 1,
      anon_sym_If,
    ACTIONS(23), 1,
      anon_sym_LPAREN,
    ACTIONS(25), 1,
      anon_sym_Private,
    ACTIONS(27), 1,
      anon_sym_Dim,
    ACTIONS(29), 1,
      anon_sym_ReDim,
    ACTIONS(31), 1,
      anon_sym_New,
    ACTIONS(35), 1,
      anon_sym_Call,
    ACTIONS(37), 1,
      sym_number,
    ACTIONS(39), 1,
      anon_sym_DQUOTE,
    ACTIONS(43), 1,
      sym_identifier,
    ACTIONS(101), 1,
      ts_builtin_sym_end,
    STATE(2), 1,
      aux_sym_source_file_repeat1,
    STATE(311), 1,
      sym_array_element,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(33), 2,
      anon_sym_DASH,
      anon_sym_Not,
    ACTIONS(41), 2,
      anon_sym_True,
      anon_sym_False,
    STATE(46), 2,
      sym_string_literal,
      sym_boolean,
    STATE(32), 7,
      sym__expression,
      sym_new_expression,
      sym_member_expression,
      sym_binary_expression,
      sym_unary_expression,
      sym_function_call,
      sym_literal,
    STATE(236), 14,
      sym__statement,
      sym__inline_statement,
      sym_variable_assignment,
      sym_invocation_statement,
      sym_exit_statement,
      sym_if_statement,
      sym_for_statement,
      sym_while_statement,
      sym_do_statement,
      sym_subroutine,
      sym_function,
      sym_variable_declaration,
      sym_redim,
      sym_ptrsafe_function_declaration,
  [198] = 24,
    ACTIONS(9), 1,
      anon_sym_Exit,
    ACTIONS(11), 1,
      anon_sym_For,
    ACTIONS(17), 1,
      anon_sym_Do,
    ACTIONS(19), 1,
      anon_sym_While,
    ACTIONS(21), 1,
      anon_sym_If,
    ACTIONS(23), 1,
      anon_sym_LPAREN,
    ACTIONS(27), 1,
      anon_sym_Dim,
    ACTIONS(29), 1,
      anon_sym_ReDim,
    ACTIONS(31), 1,
      anon_sym_New,
    ACTIONS(35), 1,
      anon_sym_Call,
    ACTIONS(37), 1,
      sym_number,
    ACTIONS(39), 1,
      anon_sym_DQUOTE,
    ACTIONS(43), 1,
      sym_identifier,
    ACTIONS(103), 1,
      aux_sym__whitespace_token1,
    STATE(30), 1,
      aux_sym__inline_statement_block_repeat1,
    STATE(89), 1,
      aux_sym__whitespace,
    STATE(298), 1,
      sym__inline_statement_block,
    STATE(311), 1,
      sym_array_element,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(33), 2,
      anon_sym_DASH,
      anon_sym_Not,
    ACTIONS(41), 2,
      anon_sym_True,
      anon_sym_False,
    STATE(46), 2,
      sym_string_literal,
      sym_boolean,
    STATE(32), 7,
      sym__expression,
      sym_new_expression,
      sym_member_expression,
      sym_binary_expression,
      sym_unary_expression,
      sym_function_call,
      sym_literal,
    STATE(231), 10,
      sym__inline_statement,
      sym_variable_assignment,
      sym_invocation_statement,
      sym_exit_statement,
      sym_if_statement,
      sym_for_statement,
      sym_while_statement,
      sym_do_statement,
      sym_variable_declaration,
      sym_redim,
  [290] = 24,
    ACTIONS(9), 1,
      anon_sym_Exit,
    ACTIONS(11), 1,
      anon_sym_For,
    ACTIONS(17), 1,
      anon_sym_Do,
    ACTIONS(19), 1,
      anon_sym_While,
    ACTIONS(21), 1,
      anon_sym_If,
    ACTIONS(23), 1,
      anon_sym_LPAREN,
    ACTIONS(27), 1,
      anon_sym_Dim,
    ACTIONS(29), 1,
      anon_sym_ReDim,
    ACTIONS(31), 1,
      anon_sym_New,
    ACTIONS(35), 1,
      anon_sym_Call,
    ACTIONS(37), 1,
      sym_number,
    ACTIONS(39), 1,
      anon_sym_DQUOTE,
    ACTIONS(43), 1,
      sym_identifier,
    ACTIONS(105), 1,
      aux_sym__whitespace_token1,
    STATE(20), 1,
      aux_sym__inline_statement_block_repeat1,
    STATE(84), 1,
      aux_sym__whitespace,
    STATE(293), 1,
      sym__inline_statement_block,
    STATE(311), 1,
      sym_array_element,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(33), 2,
      anon_sym_DASH,
      anon_sym_Not,
    ACTIONS(41), 2,
      anon_sym_True,
      anon_sym_False,
    STATE(46), 2,
      sym_string_literal,
      sym_boolean,
    STATE(32), 7,
      sym__expression,
      sym_new_expression,
      sym_member_expression,
      sym_binary_expression,
      sym_unary_expression,
      sym_function_call,
      sym_literal,
    STATE(241), 10,
      sym__inline_statement,
      sym_variable_assignment,
      sym_invocation_statement,
      sym_exit_statement,
      sym_if_statement,
      sym_for_statement,
      sym_while_statement,
      sym_do_statement,
      sym_variable_declaration,
      sym_redim,
  [382] = 24,
    ACTIONS(9), 1,
      anon_sym_Exit,
    ACTIONS(11), 1,
      anon_sym_For,
    ACTIONS(17), 1,
      anon_sym_Do,
    ACTIONS(19), 1,
      anon_sym_While,
    ACTIONS(21), 1,
      anon_sym_If,
    ACTIONS(23), 1,
      anon_sym_LPAREN,
    ACTIONS(27), 1,
      anon_sym_Dim,
    ACTIONS(29), 1,
      anon_sym_ReDim,
    ACTIONS(31), 1,
      anon_sym_New,
    ACTIONS(35), 1,
      anon_sym_Call,
    ACTIONS(37), 1,
      sym_number,
    ACTIONS(39), 1,
      anon_sym_DQUOTE,
    ACTIONS(43), 1,
      sym_identifier,
    ACTIONS(107), 1,
      aux_sym__whitespace_token1,
    STATE(31), 1,
      aux_sym__inline_statement_block_repeat1,
    STATE(109), 1,
      aux_sym__whitespace,
    STATE(263), 1,
      sym__inline_statement_block,
    STATE(311), 1,
      sym_array_element,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(33), 2,
      anon_sym_DASH,
      anon_sym_Not,
    ACTIONS(41), 2,
      anon_sym_True,
      anon_sym_False,
    STATE(46), 2,
      sym_string_literal,
      sym_boolean,
    STATE(32), 7,
      sym__expression,
      sym_new_expression,
      sym_member_expression,
      sym_binary_expression,
      sym_unary_expression,
      sym_function_call,
      sym_literal,
    STATE(233), 10,
      sym__inline_statement,
      sym_variable_assignment,
      sym_invocation_statement,
      sym_exit_statement,
      sym_if_statement,
      sym_for_statement,
      sym_while_statement,
      sym_do_statement,
      sym_variable_declaration,
      sym_redim,
  [474] = 24,
    ACTIONS(9), 1,
      anon_sym_Exit,
    ACTIONS(11), 1,
      anon_sym_For,
    ACTIONS(17), 1,
      anon_sym_Do,
    ACTIONS(19), 1,
      anon_sym_While,
    ACTIONS(21), 1,
      anon_sym_If,
    ACTIONS(23), 1,
      anon_sym_LPAREN,
    ACTIONS(27), 1,
      anon_sym_Dim,
    ACTIONS(29), 1,
      anon_sym_ReDim,
    ACTIONS(31), 1,
      anon_sym_New,
    ACTIONS(35), 1,
      anon_sym_Call,
    ACTIONS(37), 1,
      sym_number,
    ACTIONS(39), 1,
      anon_sym_DQUOTE,
    ACTIONS(43), 1,
      sym_identifier,
    ACTIONS(109), 1,
      aux_sym__whitespace_token1,
    STATE(18), 1,
      aux_sym__inline_statement_block_repeat1,
    STATE(69), 1,
      aux_sym__whitespace,
    STATE(222), 1,
      sym__inline_statement_block,
    STATE(311), 1,
      sym_array_element,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(33), 2,
      anon_sym_DASH,
      anon_sym_Not,
    ACTIONS(41), 2,
      anon_sym_True,
      anon_sym_False,
    STATE(46), 2,
      sym_string_literal,
      sym_boolean,
    STATE(32), 7,
      sym__expression,
      sym_new_expression,
      sym_member_expression,
      sym_binary_expression,
      sym_unary_expression,
      sym_function_call,
      sym_literal,
    STATE(235), 10,
      sym__inline_statement,
      sym_variable_assignment,
      sym_invocation_statement,
      sym_exit_statement,
      sym_if_statement,
      sym_for_statement,
      sym_while_statement,
      sym_do_statement,
      sym_variable_declaration,
      sym_redim,
  [566] = 24,
    ACTIONS(9), 1,
      anon_sym_Exit,
    ACTIONS(11), 1,
      anon_sym_For,
    ACTIONS(17), 1,
      anon_sym_Do,
    ACTIONS(19), 1,
      anon_sym_While,
    ACTIONS(21), 1,
      anon_sym_If,
    ACTIONS(23), 1,
      anon_sym_LPAREN,
    ACTIONS(27), 1,
      anon_sym_Dim,
    ACTIONS(29), 1,
      anon_sym_ReDim,
    ACTIONS(31), 1,
      anon_sym_New,
    ACTIONS(35), 1,
      anon_sym_Call,
    ACTIONS(37), 1,
      sym_number,
    ACTIONS(39), 1,
      anon_sym_DQUOTE,
    ACTIONS(43), 1,
      sym_identifier,
    ACTIONS(111), 1,
      aux_sym__whitespace_token1,
    STATE(26), 1,
      aux_sym__inline_statement_block_repeat1,
    STATE(73), 1,
      aux_sym__whitespace,
    STATE(289), 1,
      sym__inline_statement_block,
    STATE(311), 1,
      sym_array_element,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(33), 2,
      anon_sym_DASH,
      anon_sym_Not,
    ACTIONS(41), 2,
      anon_sym_True,
      anon_sym_False,
    STATE(46), 2,
      sym_string_literal,
      sym_boolean,
    STATE(32), 7,
      sym__expression,
      sym_new_expression,
      sym_member_expression,
      sym_binary_expression,
      sym_unary_expression,
      sym_function_call,
      sym_literal,
    STATE(230), 10,
      sym__inline_statement,
      sym_variable_assignment,
      sym_invocation_statement,
      sym_exit_statement,
      sym_if_statement,
      sym_for_statement,
      sym_while_statement,
      sym_do_statement,
      sym_variable_declaration,
      sym_redim,
  [658] = 24,
    ACTIONS(9), 1,
      anon_sym_Exit,
    ACTIONS(11), 1,
      anon_sym_For,
    ACTIONS(17), 1,
      anon_sym_Do,
    ACTIONS(19), 1,
      anon_sym_While,
    ACTIONS(21), 1,
      anon_sym_If,
    ACTIONS(23), 1,
      anon_sym_LPAREN,
    ACTIONS(27), 1,
      anon_sym_Dim,
    ACTIONS(29), 1,
      anon_sym_ReDim,
    ACTIONS(31), 1,
      anon_sym_New,
    ACTIONS(35), 1,
      anon_sym_Call,
    ACTIONS(37), 1,
      sym_number,
    ACTIONS(39), 1,
      anon_sym_DQUOTE,
    ACTIONS(43), 1,
      sym_identifier,
    ACTIONS(113), 1,
      aux_sym__whitespace_token1,
    STATE(27), 1,
      aux_sym__inline_statement_block_repeat1,
    STATE(98), 1,
      aux_sym__whitespace,
    STATE(283), 1,
      sym__inline_statement_block,
    STATE(311), 1,
      sym_array_element,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(33), 2,
      anon_sym_DASH,
      anon_sym_Not,
    ACTIONS(41), 2,
      anon_sym_True,
      anon_sym_False,
    STATE(46), 2,
      sym_string_literal,
      sym_boolean,
    STATE(32), 7,
      sym__expression,
      sym_new_expression,
      sym_member_expression,
      sym_binary_expression,
      sym_unary_expression,
      sym_function_call,
      sym_literal,
    STATE(234), 10,
      sym__inline_statement,
      sym_variable_assignment,
      sym_invocation_statement,
      sym_exit_statement,
      sym_if_statement,
      sym_for_statement,
      sym_while_statement,
      sym_do_statement,
      sym_variable_declaration,
      sym_redim,
  [750] = 24,
    ACTIONS(9), 1,
      anon_sym_Exit,
    ACTIONS(11), 1,
      anon_sym_For,
    ACTIONS(17), 1,
      anon_sym_Do,
    ACTIONS(19), 1,
      anon_sym_While,
    ACTIONS(21), 1,
      anon_sym_If,
    ACTIONS(23), 1,
      anon_sym_LPAREN,
    ACTIONS(27), 1,
      anon_sym_Dim,
    ACTIONS(29), 1,
      anon_sym_ReDim,
    ACTIONS(31), 1,
      anon_sym_New,
    ACTIONS(35), 1,
      anon_sym_Call,
    ACTIONS(37), 1,
      sym_number,
    ACTIONS(39), 1,
      anon_sym_DQUOTE,
    ACTIONS(43), 1,
      sym_identifier,
    ACTIONS(113), 1,
      aux_sym__whitespace_token1,
    STATE(27), 1,
      aux_sym__inline_statement_block_repeat1,
    STATE(98), 1,
      aux_sym__whitespace,
    STATE(270), 1,
      sym__inline_statement_block,
    STATE(311), 1,
      sym_array_element,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(33), 2,
      anon_sym_DASH,
      anon_sym_Not,
    ACTIONS(41), 2,
      anon_sym_True,
      anon_sym_False,
    STATE(46), 2,
      sym_string_literal,
      sym_boolean,
    STATE(32), 7,
      sym__expression,
      sym_new_expression,
      sym_member_expression,
      sym_binary_expression,
      sym_unary_expression,
      sym_function_call,
      sym_literal,
    STATE(234), 10,
      sym__inline_statement,
      sym_variable_assignment,
      sym_invocation_statement,
      sym_exit_statement,
      sym_if_statement,
      sym_for_statement,
      sym_while_statement,
      sym_do_statement,
      sym_variable_declaration,
      sym_redim,
  [842] = 24,
    ACTIONS(9), 1,
      anon_sym_Exit,
    ACTIONS(11), 1,
      anon_sym_For,
    ACTIONS(17), 1,
      anon_sym_Do,
    ACTIONS(19), 1,
      anon_sym_While,
    ACTIONS(21), 1,
      anon_sym_If,
    ACTIONS(23), 1,
      anon_sym_LPAREN,
    ACTIONS(27), 1,
      anon_sym_Dim,
    ACTIONS(29), 1,
      anon_sym_ReDim,
    ACTIONS(31), 1,
      anon_sym_New,
    ACTIONS(35), 1,
      anon_sym_Call,
    ACTIONS(37), 1,
      sym_number,
    ACTIONS(39), 1,
      anon_sym_DQUOTE,
    ACTIONS(43), 1,
      sym_identifier,
    ACTIONS(107), 1,
      aux_sym__whitespace_token1,
    STATE(31), 1,
      aux_sym__inline_statement_block_repeat1,
    STATE(109), 1,
      aux_sym__whitespace,
    STATE(269), 1,
      sym__inline_statement_block,
    STATE(311), 1,
      sym_array_element,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(33), 2,
      anon_sym_DASH,
      anon_sym_Not,
    ACTIONS(41), 2,
      anon_sym_True,
      anon_sym_False,
    STATE(46), 2,
      sym_string_literal,
      sym_boolean,
    STATE(32), 7,
      sym__expression,
      sym_new_expression,
      sym_member_expression,
      sym_binary_expression,
      sym_unary_expression,
      sym_function_call,
      sym_literal,
    STATE(233), 10,
      sym__inline_statement,
      sym_variable_assignment,
      sym_invocation_statement,
      sym_exit_statement,
      sym_if_statement,
      sym_for_statement,
      sym_while_statement,
      sym_do_statement,
      sym_variable_declaration,
      sym_redim,
  [934] = 24,
    ACTIONS(9), 1,
      anon_sym_Exit,
    ACTIONS(11), 1,
      anon_sym_For,
    ACTIONS(17), 1,
      anon_sym_Do,
    ACTIONS(19), 1,
      anon_sym_While,
    ACTIONS(21), 1,
      anon_sym_If,
    ACTIONS(23), 1,
      anon_sym_LPAREN,
    ACTIONS(27), 1,
      anon_sym_Dim,
    ACTIONS(29), 1,
      anon_sym_ReDim,
    ACTIONS(31), 1,
      anon_sym_New,
    ACTIONS(35), 1,
      anon_sym_Call,
    ACTIONS(37), 1,
      sym_number,
    ACTIONS(39), 1,
      anon_sym_DQUOTE,
    ACTIONS(43), 1,
      sym_identifier,
    ACTIONS(103), 1,
      aux_sym__whitespace_token1,
    STATE(30), 1,
      aux_sym__inline_statement_block_repeat1,
    STATE(89), 1,
      aux_sym__whitespace,
    STATE(311), 1,
      sym_array_element,
    STATE(314), 1,
      sym__inline_statement_block,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(33), 2,
      anon_sym_DASH,
      anon_sym_Not,
    ACTIONS(41), 2,
      anon_sym_True,
      anon_sym_False,
    STATE(46), 2,
      sym_string_literal,
      sym_boolean,
    STATE(32), 7,
      sym__expression,
      sym_new_expression,
      sym_member_expression,
      sym_binary_expression,
      sym_unary_expression,
      sym_function_call,
      sym_literal,
    STATE(231), 10,
      sym__inline_statement,
      sym_variable_assignment,
      sym_invocation_statement,
      sym_exit_statement,
      sym_if_statement,
      sym_for_statement,
      sym_while_statement,
      sym_do_statement,
      sym_variable_declaration,
      sym_redim,
  [1026] = 24,
    ACTIONS(9), 1,
      anon_sym_Exit,
    ACTIONS(11), 1,
      anon_sym_For,
    ACTIONS(17), 1,
      anon_sym_Do,
    ACTIONS(19), 1,
      anon_sym_While,
    ACTIONS(21), 1,
      anon_sym_If,
    ACTIONS(23), 1,
      anon_sym_LPAREN,
    ACTIONS(27), 1,
      anon_sym_Dim,
    ACTIONS(29), 1,
      anon_sym_ReDim,
    ACTIONS(31), 1,
      anon_sym_New,
    ACTIONS(35), 1,
      anon_sym_Call,
    ACTIONS(37), 1,
      sym_number,
    ACTIONS(39), 1,
      anon_sym_DQUOTE,
    ACTIONS(43), 1,
      sym_identifier,
    ACTIONS(115), 1,
      aux_sym__whitespace_token1,
    STATE(22), 1,
      aux_sym__inline_statement_block_repeat1,
    STATE(72), 1,
      aux_sym__whitespace,
    STATE(280), 1,
      sym__inline_statement_block,
    STATE(311), 1,
      sym_array_element,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(33), 2,
      anon_sym_DASH,
      anon_sym_Not,
    ACTIONS(41), 2,
      anon_sym_True,
      anon_sym_False,
    STATE(46), 2,
      sym_string_literal,
      sym_boolean,
    STATE(32), 7,
      sym__expression,
      sym_new_expression,
      sym_member_expression,
      sym_binary_expression,
      sym_unary_expression,
      sym_function_call,
      sym_literal,
    STATE(238), 10,
      sym__inline_statement,
      sym_variable_assignment,
      sym_invocation_statement,
      sym_exit_statement,
      sym_if_statement,
      sym_for_statement,
      sym_while_statement,
      sym_do_statement,
      sym_variable_declaration,
      sym_redim,
  [1118] = 24,
    ACTIONS(9), 1,
      anon_sym_Exit,
    ACTIONS(11), 1,
      anon_sym_For,
    ACTIONS(17), 1,
      anon_sym_Do,
    ACTIONS(19), 1,
      anon_sym_While,
    ACTIONS(21), 1,
      anon_sym_If,
    ACTIONS(23), 1,
      anon_sym_LPAREN,
    ACTIONS(27), 1,
      anon_sym_Dim,
    ACTIONS(29), 1,
      anon_sym_ReDim,
    ACTIONS(31), 1,
      anon_sym_New,
    ACTIONS(35), 1,
      anon_sym_Call,
    ACTIONS(37), 1,
      sym_number,
    ACTIONS(39), 1,
      anon_sym_DQUOTE,
    ACTIONS(43), 1,
      sym_identifier,
    ACTIONS(113), 1,
      aux_sym__whitespace_token1,
    STATE(27), 1,
      aux_sym__inline_statement_block_repeat1,
    STATE(98), 1,
      aux_sym__whitespace,
    STATE(266), 1,
      sym__inline_statement_block,
    STATE(311), 1,
      sym_array_element,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(33), 2,
      anon_sym_DASH,
      anon_sym_Not,
    ACTIONS(41), 2,
      anon_sym_True,
      anon_sym_False,
    STATE(46), 2,
      sym_string_literal,
      sym_boolean,
    STATE(32), 7,
      sym__expression,
      sym_new_expression,
      sym_member_expression,
      sym_binary_expression,
      sym_unary_expression,
      sym_function_call,
      sym_literal,
    STATE(234), 10,
      sym__inline_statement,
      sym_variable_assignment,
      sym_invocation_statement,
      sym_exit_statement,
      sym_if_statement,
      sym_for_statement,
      sym_while_statement,
      sym_do_statement,
      sym_variable_declaration,
      sym_redim,
  [1210] = 24,
    ACTIONS(9), 1,
      anon_sym_Exit,
    ACTIONS(11), 1,
      anon_sym_For,
    ACTIONS(17), 1,
      anon_sym_Do,
    ACTIONS(19), 1,
      anon_sym_While,
    ACTIONS(21), 1,
      anon_sym_If,
    ACTIONS(23), 1,
      anon_sym_LPAREN,
    ACTIONS(27), 1,
      anon_sym_Dim,
    ACTIONS(29), 1,
      anon_sym_ReDim,
    ACTIONS(31), 1,
      anon_sym_New,
    ACTIONS(35), 1,
      anon_sym_Call,
    ACTIONS(37), 1,
      sym_number,
    ACTIONS(39), 1,
      anon_sym_DQUOTE,
    ACTIONS(43), 1,
      sym_identifier,
    ACTIONS(113), 1,
      aux_sym__whitespace_token1,
    STATE(27), 1,
      aux_sym__inline_statement_block_repeat1,
    STATE(98), 1,
      aux_sym__whitespace,
    STATE(302), 1,
      sym__inline_statement_block,
    STATE(311), 1,
      sym_array_element,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(33), 2,
      anon_sym_DASH,
      anon_sym_Not,
    ACTIONS(41), 2,
      anon_sym_True,
      anon_sym_False,
    STATE(46), 2,
      sym_string_literal,
      sym_boolean,
    STATE(32), 7,
      sym__expression,
      sym_new_expression,
      sym_member_expression,
      sym_binary_expression,
      sym_unary_expression,
      sym_function_call,
      sym_literal,
    STATE(234), 10,
      sym__inline_statement,
      sym_variable_assignment,
      sym_invocation_statement,
      sym_exit_statement,
      sym_if_statement,
      sym_for_statement,
      sym_while_statement,
      sym_do_statement,
      sym_variable_declaration,
      sym_redim,
  [1302] = 16,
    ACTIONS(117), 1,
      anon_sym_LPAREN,
    ACTIONS(121), 1,
      anon_sym_New,
    ACTIONS(125), 1,
      anon_sym_Call,
    ACTIONS(127), 1,
      sym_number,
    ACTIONS(129), 1,
      anon_sym_DQUOTE,
    ACTIONS(133), 1,
      sym_identifier,
    STATE(173), 1,
      sym_member_expression,
    STATE(217), 1,
      sym_type,
    STATE(218), 1,
      sym_type_member_expression,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(123), 2,
      anon_sym_DASH,
      anon_sym_Not,
    ACTIONS(131), 2,
      anon_sym_True,
      anon_sym_False,
    STATE(92), 2,
      sym_string_literal,
      sym_boolean,
    STATE(215), 2,
      sym_type_terminal,
      sym_array_type,
    STATE(174), 6,
      sym__expression,
      sym_new_expression,
      sym_binary_expression,
      sym_unary_expression,
      sym_function_call,
      sym_literal,
    ACTIONS(119), 17,
      anon_sym_Any,
      anon_sym_Boolean,
      anon_sym_Byte,
      anon_sym_Collection,
      anon_sym_Currency,
      anon_sym_Date,
      anon_sym_Decimal,
      anon_sym_Dictionary,
      anon_sym_Double,
      anon_sym_Integer,
      anon_sym_Long,
      anon_sym_LongLong,
      anon_sym_LongPtr,
      anon_sym_Object,
      anon_sym_Single,
      anon_sym_String,
      anon_sym_Variant,
  [1377] = 16,
    ACTIONS(117), 1,
      anon_sym_LPAREN,
    ACTIONS(121), 1,
      anon_sym_New,
    ACTIONS(125), 1,
      anon_sym_Call,
    ACTIONS(127), 1,
      sym_number,
    ACTIONS(129), 1,
      anon_sym_DQUOTE,
    ACTIONS(137), 1,
      sym_identifier,
    STATE(176), 1,
      sym_member_expression,
    STATE(212), 1,
      sym_type_member_expression,
    STATE(213), 1,
      sym_type,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(123), 2,
      anon_sym_DASH,
      anon_sym_Not,
    ACTIONS(131), 2,
      anon_sym_True,
      anon_sym_False,
    STATE(92), 2,
      sym_string_literal,
      sym_boolean,
    STATE(204), 2,
      sym_type_terminal,
      sym_array_type,
    STATE(174), 6,
      sym__expression,
      sym_new_expression,
      sym_binary_expression,
      sym_unary_expression,
      sym_function_call,
      sym_literal,
    ACTIONS(135), 17,
      anon_sym_Any,
      anon_sym_Boolean,
      anon_sym_Byte,
      anon_sym_Collection,
      anon_sym_Currency,
      anon_sym_Date,
      anon_sym_Decimal,
      anon_sym_Dictionary,
      anon_sym_Double,
      anon_sym_Integer,
      anon_sym_Long,
      anon_sym_LongLong,
      anon_sym_LongPtr,
      anon_sym_Object,
      anon_sym_Single,
      anon_sym_String,
      anon_sym_Variant,
  [1452] = 22,
    ACTIONS(9), 1,
      anon_sym_Exit,
    ACTIONS(11), 1,
      anon_sym_For,
    ACTIONS(17), 1,
      anon_sym_Do,
    ACTIONS(19), 1,
      anon_sym_While,
    ACTIONS(21), 1,
      anon_sym_If,
    ACTIONS(23), 1,
      anon_sym_LPAREN,
    ACTIONS(27), 1,
      anon_sym_Dim,
    ACTIONS(29), 1,
      anon_sym_ReDim,
    ACTIONS(31), 1,
      anon_sym_New,
    ACTIONS(35), 1,
      anon_sym_Call,
    ACTIONS(37), 1,
      sym_number,
    ACTIONS(39), 1,
      anon_sym_DQUOTE,
    ACTIONS(43), 1,
      sym_identifier,
    STATE(19), 1,
      aux_sym__inline_statement_block_repeat1,
    STATE(311), 1,
      sym_array_element,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(33), 2,
      anon_sym_DASH,
      anon_sym_Not,
    ACTIONS(41), 2,
      anon_sym_True,
      anon_sym_False,
    ACTIONS(139), 2,
      anon_sym_Else,
      anon_sym_EndIf,
    STATE(46), 2,
      sym_string_literal,
      sym_boolean,
    STATE(32), 7,
      sym__expression,
      sym_new_expression,
      sym_member_expression,
      sym_binary_expression,
      sym_unary_expression,
      sym_function_call,
      sym_literal,
    STATE(235), 10,
      sym__inline_statement,
      sym_variable_assignment,
      sym_invocation_statement,
      sym_exit_statement,
      sym_if_statement,
      sym_for_statement,
      sym_while_statement,
      sym_do_statement,
      sym_variable_declaration,
      sym_redim,
  [1539] = 22,
    ACTIONS(141), 1,
      anon_sym_Exit,
    ACTIONS(144), 1,
      anon_sym_For,
    ACTIONS(147), 1,
      anon_sym_Do,
    ACTIONS(150), 1,
      anon_sym_While,
    ACTIONS(153), 1,
      anon_sym_If,
    ACTIONS(158), 1,
      anon_sym_LPAREN,
    ACTIONS(161), 1,
      anon_sym_Dim,
    ACTIONS(164), 1,
      anon_sym_ReDim,
    ACTIONS(167), 1,
      anon_sym_New,
    ACTIONS(173), 1,
      anon_sym_Call,
    ACTIONS(176), 1,
      sym_number,
    ACTIONS(179), 1,
      anon_sym_DQUOTE,
    ACTIONS(185), 1,
      sym_identifier,
    STATE(19), 1,
      aux_sym__inline_statement_block_repeat1,
    STATE(311), 1,
      sym_array_element,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(156), 2,
      anon_sym_Else,
      anon_sym_EndIf,
    ACTIONS(170), 2,
      anon_sym_DASH,
      anon_sym_Not,
    ACTIONS(182), 2,
      anon_sym_True,
      anon_sym_False,
    STATE(46), 2,
      sym_string_literal,
      sym_boolean,
    STATE(32), 7,
      sym__expression,
      sym_new_expression,
      sym_member_expression,
      sym_binary_expression,
      sym_unary_expression,
      sym_function_call,
      sym_literal,
    STATE(235), 10,
      sym__inline_statement,
      sym_variable_assignment,
      sym_invocation_statement,
      sym_exit_statement,
      sym_if_statement,
      sym_for_statement,
      sym_while_statement,
      sym_do_statement,
      sym_variable_declaration,
      sym_redim,
  [1626] = 22,
    ACTIONS(9), 1,
      anon_sym_Exit,
    ACTIONS(11), 1,
      anon_sym_For,
    ACTIONS(17), 1,
      anon_sym_Do,
    ACTIONS(19), 1,
      anon_sym_While,
    ACTIONS(21), 1,
      anon_sym_If,
    ACTIONS(23), 1,
      anon_sym_LPAREN,
    ACTIONS(27), 1,
      anon_sym_Dim,
    ACTIONS(29), 1,
      anon_sym_ReDim,
    ACTIONS(31), 1,
      anon_sym_New,
    ACTIONS(35), 1,
      anon_sym_Call,
    ACTIONS(37), 1,
      sym_number,
    ACTIONS(39), 1,
      anon_sym_DQUOTE,
    ACTIONS(43), 1,
      sym_identifier,
    ACTIONS(139), 1,
      anon_sym_Loop,
    STATE(29), 1,
      aux_sym__inline_statement_block_repeat1,
    STATE(311), 1,
      sym_array_element,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(33), 2,
      anon_sym_DASH,
      anon_sym_Not,
    ACTIONS(41), 2,
      anon_sym_True,
      anon_sym_False,
    STATE(46), 2,
      sym_string_literal,
      sym_boolean,
    STATE(32), 7,
      sym__expression,
      sym_new_expression,
      sym_member_expression,
      sym_binary_expression,
      sym_unary_expression,
      sym_function_call,
      sym_literal,
    STATE(241), 10,
      sym__inline_statement,
      sym_variable_assignment,
      sym_invocation_statement,
      sym_exit_statement,
      sym_if_statement,
      sym_for_statement,
      sym_while_statement,
      sym_do_statement,
      sym_variable_declaration,
      sym_redim,
  [1712] = 22,
    ACTIONS(141), 1,
      anon_sym_Exit,
    ACTIONS(144), 1,
      anon_sym_For,
    ACTIONS(147), 1,
      anon_sym_Do,
    ACTIONS(150), 1,
      anon_sym_While,
    ACTIONS(153), 1,
      anon_sym_If,
    ACTIONS(156), 1,
      anon_sym_Next,
    ACTIONS(158), 1,
      anon_sym_LPAREN,
    ACTIONS(161), 1,
      anon_sym_Dim,
    ACTIONS(164), 1,
      anon_sym_ReDim,
    ACTIONS(167), 1,
      anon_sym_New,
    ACTIONS(173), 1,
      anon_sym_Call,
    ACTIONS(176), 1,
      sym_number,
    ACTIONS(179), 1,
      anon_sym_DQUOTE,
    ACTIONS(185), 1,
      sym_identifier,
    STATE(21), 1,
      aux_sym__inline_statement_block_repeat1,
    STATE(311), 1,
      sym_array_element,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(170), 2,
      anon_sym_DASH,
      anon_sym_Not,
    ACTIONS(182), 2,
      anon_sym_True,
      anon_sym_False,
    STATE(46), 2,
      sym_string_literal,
      sym_boolean,
    STATE(32), 7,
      sym__expression,
      sym_new_expression,
      sym_member_expression,
      sym_binary_expression,
      sym_unary_expression,
      sym_function_call,
      sym_literal,
    STATE(231), 10,
      sym__inline_statement,
      sym_variable_assignment,
      sym_invocation_statement,
      sym_exit_statement,
      sym_if_statement,
      sym_for_statement,
      sym_while_statement,
      sym_do_statement,
      sym_variable_declaration,
      sym_redim,
  [1798] = 22,
    ACTIONS(9), 1,
      anon_sym_Exit,
    ACTIONS(11), 1,
      anon_sym_For,
    ACTIONS(17), 1,
      anon_sym_Do,
    ACTIONS(19), 1,
      anon_sym_While,
    ACTIONS(21), 1,
      anon_sym_If,
    ACTIONS(23), 1,
      anon_sym_LPAREN,
    ACTIONS(27), 1,
      anon_sym_Dim,
    ACTIONS(29), 1,
      anon_sym_ReDim,
    ACTIONS(31), 1,
      anon_sym_New,
    ACTIONS(35), 1,
      anon_sym_Call,
    ACTIONS(37), 1,
      sym_number,
    ACTIONS(39), 1,
      anon_sym_DQUOTE,
    ACTIONS(43), 1,
      sym_identifier,
    ACTIONS(139), 1,
      anon_sym_Wend,
    STATE(25), 1,
      aux_sym__inline_statement_block_repeat1,
    STATE(311), 1,
      sym_array_element,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(33), 2,
      anon_sym_DASH,
      anon_sym_Not,
    ACTIONS(41), 2,
      anon_sym_True,
      anon_sym_False,
    STATE(46), 2,
      sym_string_literal,
      sym_boolean,
    STATE(32), 7,
      sym__expression,
      sym_new_expression,
      sym_member_expression,
      sym_binary_expression,
      sym_unary_expression,
      sym_function_call,
      sym_literal,
    STATE(238), 10,
      sym__inline_statement,
      sym_variable_assignment,
      sym_invocation_statement,
      sym_exit_statement,
      sym_if_statement,
      sym_for_statement,
      sym_while_statement,
      sym_do_statement,
      sym_variable_declaration,
      sym_redim,
  [1884] = 22,
    ACTIONS(141), 1,
      anon_sym_Exit,
    ACTIONS(144), 1,
      anon_sym_For,
    ACTIONS(147), 1,
      anon_sym_Do,
    ACTIONS(150), 1,
      anon_sym_While,
    ACTIONS(153), 1,
      anon_sym_If,
    ACTIONS(156), 1,
      anon_sym_EndFunction,
    ACTIONS(158), 1,
      anon_sym_LPAREN,
    ACTIONS(161), 1,
      anon_sym_Dim,
    ACTIONS(164), 1,
      anon_sym_ReDim,
    ACTIONS(167), 1,
      anon_sym_New,
    ACTIONS(173), 1,
      anon_sym_Call,
    ACTIONS(176), 1,
      sym_number,
    ACTIONS(179), 1,
      anon_sym_DQUOTE,
    ACTIONS(185), 1,
      sym_identifier,
    STATE(23), 1,
      aux_sym__inline_statement_block_repeat1,
    STATE(311), 1,
      sym_array_element,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(170), 2,
      anon_sym_DASH,
      anon_sym_Not,
    ACTIONS(182), 2,
      anon_sym_True,
      anon_sym_False,
    STATE(46), 2,
      sym_string_literal,
      sym_boolean,
    STATE(32), 7,
      sym__expression,
      sym_new_expression,
      sym_member_expression,
      sym_binary_expression,
      sym_unary_expression,
      sym_function_call,
      sym_literal,
    STATE(234), 10,
      sym__inline_statement,
      sym_variable_assignment,
      sym_invocation_statement,
      sym_exit_statement,
      sym_if_statement,
      sym_for_statement,
      sym_while_statement,
      sym_do_statement,
      sym_variable_declaration,
      sym_redim,
  [1970] = 22,
    ACTIONS(141), 1,
      anon_sym_Exit,
    ACTIONS(144), 1,
      anon_sym_For,
    ACTIONS(147), 1,
      anon_sym_Do,
    ACTIONS(150), 1,
      anon_sym_While,
    ACTIONS(153), 1,
      anon_sym_If,
    ACTIONS(156), 1,
      anon_sym_EndIf,
    ACTIONS(158), 1,
      anon_sym_LPAREN,
    ACTIONS(161), 1,
      anon_sym_Dim,
    ACTIONS(164), 1,
      anon_sym_ReDim,
    ACTIONS(167), 1,
      anon_sym_New,
    ACTIONS(173), 1,
      anon_sym_Call,
    ACTIONS(176), 1,
      sym_number,
    ACTIONS(179), 1,
      anon_sym_DQUOTE,
    ACTIONS(185), 1,
      sym_identifier,
    STATE(24), 1,
      aux_sym__inline_statement_block_repeat1,
    STATE(311), 1,
      sym_array_element,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(170), 2,
      anon_sym_DASH,
      anon_sym_Not,
    ACTIONS(182), 2,
      anon_sym_True,
      anon_sym_False,
    STATE(46), 2,
      sym_string_literal,
      sym_boolean,
    STATE(32), 7,
      sym__expression,
      sym_new_expression,
      sym_member_expression,
      sym_binary_expression,
      sym_unary_expression,
      sym_function_call,
      sym_literal,
    STATE(230), 10,
      sym__inline_statement,
      sym_variable_assignment,
      sym_invocation_statement,
      sym_exit_statement,
      sym_if_statement,
      sym_for_statement,
      sym_while_statement,
      sym_do_statement,
      sym_variable_declaration,
      sym_redim,
  [2056] = 22,
    ACTIONS(141), 1,
      anon_sym_Exit,
    ACTIONS(144), 1,
      anon_sym_For,
    ACTIONS(147), 1,
      anon_sym_Do,
    ACTIONS(150), 1,
      anon_sym_While,
    ACTIONS(153), 1,
      anon_sym_If,
    ACTIONS(156), 1,
      anon_sym_Wend,
    ACTIONS(158), 1,
      anon_sym_LPAREN,
    ACTIONS(161), 1,
      anon_sym_Dim,
    ACTIONS(164), 1,
      anon_sym_ReDim,
    ACTIONS(167), 1,
      anon_sym_New,
    ACTIONS(173), 1,
      anon_sym_Call,
    ACTIONS(176), 1,
      sym_number,
    ACTIONS(179), 1,
      anon_sym_DQUOTE,
    ACTIONS(185), 1,
      sym_identifier,
    STATE(25), 1,
      aux_sym__inline_statement_block_repeat1,
    STATE(311), 1,
      sym_array_element,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(170), 2,
      anon_sym_DASH,
      anon_sym_Not,
    ACTIONS(182), 2,
      anon_sym_True,
      anon_sym_False,
    STATE(46), 2,
      sym_string_literal,
      sym_boolean,
    STATE(32), 7,
      sym__expression,
      sym_new_expression,
      sym_member_expression,
      sym_binary_expression,
      sym_unary_expression,
      sym_function_call,
      sym_literal,
    STATE(238), 10,
      sym__inline_statement,
      sym_variable_assignment,
      sym_invocation_statement,
      sym_exit_statement,
      sym_if_statement,
      sym_for_statement,
      sym_while_statement,
      sym_do_statement,
      sym_variable_declaration,
      sym_redim,
  [2142] = 22,
    ACTIONS(9), 1,
      anon_sym_Exit,
    ACTIONS(11), 1,
      anon_sym_For,
    ACTIONS(17), 1,
      anon_sym_Do,
    ACTIONS(19), 1,
      anon_sym_While,
    ACTIONS(21), 1,
      anon_sym_If,
    ACTIONS(23), 1,
      anon_sym_LPAREN,
    ACTIONS(27), 1,
      anon_sym_Dim,
    ACTIONS(29), 1,
      anon_sym_ReDim,
    ACTIONS(31), 1,
      anon_sym_New,
    ACTIONS(35), 1,
      anon_sym_Call,
    ACTIONS(37), 1,
      sym_number,
    ACTIONS(39), 1,
      anon_sym_DQUOTE,
    ACTIONS(43), 1,
      sym_identifier,
    ACTIONS(139), 1,
      anon_sym_EndIf,
    STATE(24), 1,
      aux_sym__inline_statement_block_repeat1,
    STATE(311), 1,
      sym_array_element,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(33), 2,
      anon_sym_DASH,
      anon_sym_Not,
    ACTIONS(41), 2,
      anon_sym_True,
      anon_sym_False,
    STATE(46), 2,
      sym_string_literal,
      sym_boolean,
    STATE(32), 7,
      sym__expression,
      sym_new_expression,
      sym_member_expression,
      sym_binary_expression,
      sym_unary_expression,
      sym_function_call,
      sym_literal,
    STATE(230), 10,
      sym__inline_statement,
      sym_variable_assignment,
      sym_invocation_statement,
      sym_exit_statement,
      sym_if_statement,
      sym_for_statement,
      sym_while_statement,
      sym_do_statement,
      sym_variable_declaration,
      sym_redim,
  [2228] = 22,
    ACTIONS(9), 1,
      anon_sym_Exit,
    ACTIONS(11), 1,
      anon_sym_For,
    ACTIONS(17), 1,
      anon_sym_Do,
    ACTIONS(19), 1,
      anon_sym_While,
    ACTIONS(21), 1,
      anon_sym_If,
    ACTIONS(23), 1,
      anon_sym_LPAREN,
    ACTIONS(27), 1,
      anon_sym_Dim,
    ACTIONS(29), 1,
      anon_sym_ReDim,
    ACTIONS(31), 1,
      anon_sym_New,
    ACTIONS(35), 1,
      anon_sym_Call,
    ACTIONS(37), 1,
      sym_number,
    ACTIONS(39), 1,
      anon_sym_DQUOTE,
    ACTIONS(43), 1,
      sym_identifier,
    ACTIONS(139), 1,
      anon_sym_EndFunction,
    STATE(23), 1,
      aux_sym__inline_statement_block_repeat1,
    STATE(311), 1,
      sym_array_element,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(33), 2,
      anon_sym_DASH,
      anon_sym_Not,
    ACTIONS(41), 2,
      anon_sym_True,
      anon_sym_False,
    STATE(46), 2,
      sym_string_literal,
      sym_boolean,
    STATE(32), 7,
      sym__expression,
      sym_new_expression,
      sym_member_expression,
      sym_binary_expression,
      sym_unary_expression,
      sym_function_call,
      sym_literal,
    STATE(234), 10,
      sym__inline_statement,
      sym_variable_assignment,
      sym_invocation_statement,
      sym_exit_statement,
      sym_if_statement,
      sym_for_statement,
      sym_while_statement,
      sym_do_statement,
      sym_variable_declaration,
      sym_redim,
  [2314] = 22,
    ACTIONS(141), 1,
      anon_sym_Exit,
    ACTIONS(144), 1,
      anon_sym_For,
    ACTIONS(147), 1,
      anon_sym_Do,
    ACTIONS(150), 1,
      anon_sym_While,
    ACTIONS(153), 1,
      anon_sym_If,
    ACTIONS(156), 1,
      anon_sym_EndSub,
    ACTIONS(158), 1,
      anon_sym_LPAREN,
    ACTIONS(161), 1,
      anon_sym_Dim,
    ACTIONS(164), 1,
      anon_sym_ReDim,
    ACTIONS(167), 1,
      anon_sym_New,
    ACTIONS(173), 1,
      anon_sym_Call,
    ACTIONS(176), 1,
      sym_number,
    ACTIONS(179), 1,
      anon_sym_DQUOTE,
    ACTIONS(185), 1,
      sym_identifier,
    STATE(28), 1,
      aux_sym__inline_statement_block_repeat1,
    STATE(311), 1,
      sym_array_element,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(170), 2,
      anon_sym_DASH,
      anon_sym_Not,
    ACTIONS(182), 2,
      anon_sym_True,
      anon_sym_False,
    STATE(46), 2,
      sym_string_literal,
      sym_boolean,
    STATE(32), 7,
      sym__expression,
      sym_new_expression,
      sym_member_expression,
      sym_binary_expression,
      sym_unary_expression,
      sym_function_call,
      sym_literal,
    STATE(233), 10,
      sym__inline_statement,
      sym_variable_assignment,
      sym_invocation_statement,
      sym_exit_statement,
      sym_if_statement,
      sym_for_statement,
      sym_while_statement,
      sym_do_statement,
      sym_variable_declaration,
      sym_redim,
  [2400] = 22,
    ACTIONS(141), 1,
      anon_sym_Exit,
    ACTIONS(144), 1,
      anon_sym_For,
    ACTIONS(147), 1,
      anon_sym_Do,
    ACTIONS(150), 1,
      anon_sym_While,
    ACTIONS(153), 1,
      anon_sym_If,
    ACTIONS(156), 1,
      anon_sym_Loop,
    ACTIONS(158), 1,
      anon_sym_LPAREN,
    ACTIONS(161), 1,
      anon_sym_Dim,
    ACTIONS(164), 1,
      anon_sym_ReDim,
    ACTIONS(167), 1,
      anon_sym_New,
    ACTIONS(173), 1,
      anon_sym_Call,
    ACTIONS(176), 1,
      sym_number,
    ACTIONS(179), 1,
      anon_sym_DQUOTE,
    ACTIONS(185), 1,
      sym_identifier,
    STATE(29), 1,
      aux_sym__inline_statement_block_repeat1,
    STATE(311), 1,
      sym_array_element,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(170), 2,
      anon_sym_DASH,
      anon_sym_Not,
    ACTIONS(182), 2,
      anon_sym_True,
      anon_sym_False,
    STATE(46), 2,
      sym_string_literal,
      sym_boolean,
    STATE(32), 7,
      sym__expression,
      sym_new_expression,
      sym_member_expression,
      sym_binary_expression,
      sym_unary_expression,
      sym_function_call,
      sym_literal,
    STATE(241), 10,
      sym__inline_statement,
      sym_variable_assignment,
      sym_invocation_statement,
      sym_exit_statement,
      sym_if_statement,
      sym_for_statement,
      sym_while_statement,
      sym_do_statement,
      sym_variable_declaration,
      sym_redim,
  [2486] = 22,
    ACTIONS(9), 1,
      anon_sym_Exit,
    ACTIONS(11), 1,
      anon_sym_For,
    ACTIONS(17), 1,
      anon_sym_Do,
    ACTIONS(19), 1,
      anon_sym_While,
    ACTIONS(21), 1,
      anon_sym_If,
    ACTIONS(23), 1,
      anon_sym_LPAREN,
    ACTIONS(27), 1,
      anon_sym_Dim,
    ACTIONS(29), 1,
      anon_sym_ReDim,
    ACTIONS(31), 1,
      anon_sym_New,
    ACTIONS(35), 1,
      anon_sym_Call,
    ACTIONS(37), 1,
      sym_number,
    ACTIONS(39), 1,
      anon_sym_DQUOTE,
    ACTIONS(43), 1,
      sym_identifier,
    ACTIONS(139), 1,
      anon_sym_Next,
    STATE(21), 1,
      aux_sym__inline_statement_block_repeat1,
    STATE(311), 1,
      sym_array_element,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(33), 2,
      anon_sym_DASH,
      anon_sym_Not,
    ACTIONS(41), 2,
      anon_sym_True,
      anon_sym_False,
    STATE(46), 2,
      sym_string_literal,
      sym_boolean,
    STATE(32), 7,
      sym__expression,
      sym_new_expression,
      sym_member_expression,
      sym_binary_expression,
      sym_unary_expression,
      sym_function_call,
      sym_literal,
    STATE(231), 10,
      sym__inline_statement,
      sym_variable_assignment,
      sym_invocation_statement,
      sym_exit_statement,
      sym_if_statement,
      sym_for_statement,
      sym_while_statement,
      sym_do_statement,
      sym_variable_declaration,
      sym_redim,
  [2572] = 22,
    ACTIONS(9), 1,
      anon_sym_Exit,
    ACTIONS(11), 1,
      anon_sym_For,
    ACTIONS(17), 1,
      anon_sym_Do,
    ACTIONS(19), 1,
      anon_sym_While,
    ACTIONS(21), 1,
      anon_sym_If,
    ACTIONS(23), 1,
      anon_sym_LPAREN,
    ACTIONS(27), 1,
      anon_sym_Dim,
    ACTIONS(29), 1,
      anon_sym_ReDim,
    ACTIONS(31), 1,
      anon_sym_New,
    ACTIONS(35), 1,
      anon_sym_Call,
    ACTIONS(37), 1,
      sym_number,
    ACTIONS(39), 1,
      anon_sym_DQUOTE,
    ACTIONS(43), 1,
      sym_identifier,
    ACTIONS(139), 1,
      anon_sym_EndSub,
    STATE(28), 1,
      aux_sym__inline_statement_block_repeat1,
    STATE(311), 1,
      sym_array_element,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(33), 2,
      anon_sym_DASH,
      anon_sym_Not,
    ACTIONS(41), 2,
      anon_sym_True,
      anon_sym_False,
    STATE(46), 2,
      sym_string_literal,
      sym_boolean,
    STATE(32), 7,
      sym__expression,
      sym_new_expression,
      sym_member_expression,
      sym_binary_expression,
      sym_unary_expression,
      sym_function_call,
      sym_literal,
    STATE(233), 10,
      sym__inline_statement,
      sym_variable_assignment,
      sym_invocation_statement,
      sym_exit_statement,
      sym_if_statement,
      sym_for_statement,
      sym_while_statement,
      sym_do_statement,
      sym_variable_declaration,
      sym_redim,
  [2658] = 20,
    ACTIONS(188), 1,
      anon_sym_LPAREN,
    ACTIONS(190), 1,
      anon_sym_New,
    ACTIONS(192), 1,
      anon_sym_DOT,
    ACTIONS(194), 1,
      anon_sym_PLUS,
    ACTIONS(196), 1,
      anon_sym_DASH,
    ACTIONS(202), 1,
      anon_sym_Not,
    ACTIONS(204), 1,
      anon_sym_Call,
    ACTIONS(206), 1,
      sym_number,
    ACTIONS(208), 1,
      anon_sym_DQUOTE,
    ACTIONS(212), 1,
      sym_identifier,
    ACTIONS(214), 1,
      aux_sym__whitespace_token1,
    STATE(203), 1,
      sym_argument,
    STATE(240), 1,
      sym_keyword_argument,
    STATE(296), 1,
      sym_argument_list,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(210), 2,
      anon_sym_True,
      anon_sym_False,
    STATE(140), 2,
      sym_string_literal,
      sym_boolean,
    ACTIONS(198), 3,
      anon_sym_STAR,
      anon_sym_SLASH,
      anon_sym_Mod,
    STATE(156), 7,
      sym__expression,
      sym_new_expression,
      sym_member_expression,
      sym_binary_expression,
      sym_unary_expression,
      sym_function_call,
      sym_literal,
    ACTIONS(200), 10,
      anon_sym_AMP,
      aux_sym_binary_expression_token1,
      aux_sym_binary_expression_token2,
      aux_sym_binary_expression_token3,
      anon_sym_LT_GT,
      anon_sym_LT,
      anon_sym_GT,
      anon_sym_LT_EQ,
      anon_sym_GT_EQ,
      sym__equal,
  [2739] = 2,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(216), 27,
      anon_sym_LPAREN,
      anon_sym_COMMA,
      anon_sym_New,
      anon_sym_DOT,
      anon_sym_PLUS,
      anon_sym_DASH,
      anon_sym_STAR,
      anon_sym_SLASH,
      anon_sym_Mod,
      anon_sym_AMP,
      aux_sym_binary_expression_token1,
      aux_sym_binary_expression_token2,
      aux_sym_binary_expression_token3,
      anon_sym_LT_GT,
      anon_sym_LT,
      anon_sym_GT,
      anon_sym_LT_EQ,
      anon_sym_GT_EQ,
      anon_sym_Not,
      anon_sym_Call,
      sym_number,
      anon_sym_DQUOTE,
      anon_sym_True,
      anon_sym_False,
      sym_identifier,
      sym__equal,
      aux_sym__whitespace_token1,
  [2773] = 2,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(218), 27,
      anon_sym_LPAREN,
      anon_sym_COMMA,
      anon_sym_New,
      anon_sym_DOT,
      anon_sym_PLUS,
      anon_sym_DASH,
      anon_sym_STAR,
      anon_sym_SLASH,
      anon_sym_Mod,
      anon_sym_AMP,
      aux_sym_binary_expression_token1,
      aux_sym_binary_expression_token2,
      aux_sym_binary_expression_token3,
      anon_sym_LT_GT,
      anon_sym_LT,
      anon_sym_GT,
      anon_sym_LT_EQ,
      anon_sym_GT_EQ,
      anon_sym_Not,
      anon_sym_Call,
      sym_number,
      anon_sym_DQUOTE,
      anon_sym_True,
      anon_sym_False,
      sym_identifier,
      sym__equal,
      aux_sym__whitespace_token1,
  [2807] = 2,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(220), 27,
      anon_sym_LPAREN,
      anon_sym_COMMA,
      anon_sym_New,
      anon_sym_DOT,
      anon_sym_PLUS,
      anon_sym_DASH,
      anon_sym_STAR,
      anon_sym_SLASH,
      anon_sym_Mod,
      anon_sym_AMP,
      aux_sym_binary_expression_token1,
      aux_sym_binary_expression_token2,
      aux_sym_binary_expression_token3,
      anon_sym_LT_GT,
      anon_sym_LT,
      anon_sym_GT,
      anon_sym_LT_EQ,
      anon_sym_GT_EQ,
      anon_sym_Not,
      anon_sym_Call,
      sym_number,
      anon_sym_DQUOTE,
      anon_sym_True,
      anon_sym_False,
      sym_identifier,
      sym__equal,
      aux_sym__whitespace_token1,
  [2841] = 3,
    ACTIONS(192), 1,
      anon_sym_DOT,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(222), 26,
      anon_sym_LPAREN,
      anon_sym_COMMA,
      anon_sym_New,
      anon_sym_PLUS,
      anon_sym_DASH,
      anon_sym_STAR,
      anon_sym_SLASH,
      anon_sym_Mod,
      anon_sym_AMP,
      aux_sym_binary_expression_token1,
      aux_sym_binary_expression_token2,
      aux_sym_binary_expression_token3,
      anon_sym_LT_GT,
      anon_sym_LT,
      anon_sym_GT,
      anon_sym_LT_EQ,
      anon_sym_GT_EQ,
      anon_sym_Not,
      anon_sym_Call,
      sym_number,
      anon_sym_DQUOTE,
      anon_sym_True,
      anon_sym_False,
      sym_identifier,
      sym__equal,
      aux_sym__whitespace_token1,
  [2877] = 5,
    ACTIONS(192), 1,
      anon_sym_DOT,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(194), 2,
      anon_sym_PLUS,
      anon_sym_DASH,
    ACTIONS(198), 3,
      anon_sym_STAR,
      anon_sym_SLASH,
      anon_sym_Mod,
    ACTIONS(222), 21,
      anon_sym_LPAREN,
      anon_sym_COMMA,
      anon_sym_New,
      anon_sym_AMP,
      aux_sym_binary_expression_token1,
      aux_sym_binary_expression_token2,
      aux_sym_binary_expression_token3,
      anon_sym_LT_GT,
      anon_sym_LT,
      anon_sym_GT,
      anon_sym_LT_EQ,
      anon_sym_GT_EQ,
      anon_sym_Not,
      anon_sym_Call,
      sym_number,
      anon_sym_DQUOTE,
      anon_sym_True,
      anon_sym_False,
      sym_identifier,
      sym__equal,
      aux_sym__whitespace_token1,
  [2917] = 2,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(224), 27,
      anon_sym_LPAREN,
      anon_sym_COMMA,
      anon_sym_New,
      anon_sym_DOT,
      anon_sym_PLUS,
      anon_sym_DASH,
      anon_sym_STAR,
      anon_sym_SLASH,
      anon_sym_Mod,
      anon_sym_AMP,
      aux_sym_binary_expression_token1,
      aux_sym_binary_expression_token2,
      aux_sym_binary_expression_token3,
      anon_sym_LT_GT,
      anon_sym_LT,
      anon_sym_GT,
      anon_sym_LT_EQ,
      anon_sym_GT_EQ,
      anon_sym_Not,
      anon_sym_Call,
      sym_number,
      anon_sym_DQUOTE,
      anon_sym_True,
      anon_sym_False,
      sym_identifier,
      sym__equal,
      aux_sym__whitespace_token1,
  [2951] = 5,
    ACTIONS(192), 1,
      anon_sym_DOT,
    ACTIONS(226), 1,
      anon_sym_COMMA,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(198), 3,
      anon_sym_STAR,
      anon_sym_SLASH,
      anon_sym_Mod,
    ACTIONS(222), 22,
      anon_sym_LPAREN,
      anon_sym_New,
      anon_sym_PLUS,
      anon_sym_DASH,
      anon_sym_AMP,
      aux_sym_binary_expression_token1,
      aux_sym_binary_expression_token2,
      aux_sym_binary_expression_token3,
      anon_sym_LT_GT,
      anon_sym_LT,
      anon_sym_GT,
      anon_sym_LT_EQ,
      anon_sym_GT_EQ,
      anon_sym_Not,
      anon_sym_Call,
      sym_number,
      anon_sym_DQUOTE,
      anon_sym_True,
      anon_sym_False,
      sym_identifier,
      sym__equal,
      aux_sym__whitespace_token1,
  [2991] = 4,
    ACTIONS(192), 1,
      anon_sym_DOT,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(198), 3,
      anon_sym_STAR,
      anon_sym_SLASH,
      anon_sym_Mod,
    ACTIONS(222), 23,
      anon_sym_LPAREN,
      anon_sym_COMMA,
      anon_sym_New,
      anon_sym_PLUS,
      anon_sym_DASH,
      anon_sym_AMP,
      aux_sym_binary_expression_token1,
      aux_sym_binary_expression_token2,
      aux_sym_binary_expression_token3,
      anon_sym_LT_GT,
      anon_sym_LT,
      anon_sym_GT,
      anon_sym_LT_EQ,
      anon_sym_GT_EQ,
      anon_sym_Not,
      anon_sym_Call,
      sym_number,
      anon_sym_DQUOTE,
      anon_sym_True,
      anon_sym_False,
      sym_identifier,
      sym__equal,
      aux_sym__whitespace_token1,
  [3029] = 3,
    ACTIONS(228), 1,
      anon_sym_LPAREN,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(230), 26,
      anon_sym_COMMA,
      anon_sym_New,
      anon_sym_DOT,
      anon_sym_PLUS,
      anon_sym_DASH,
      anon_sym_STAR,
      anon_sym_SLASH,
      anon_sym_Mod,
      anon_sym_AMP,
      aux_sym_binary_expression_token1,
      aux_sym_binary_expression_token2,
      aux_sym_binary_expression_token3,
      anon_sym_LT_GT,
      anon_sym_LT,
      anon_sym_GT,
      anon_sym_LT_EQ,
      anon_sym_GT_EQ,
      anon_sym_Not,
      anon_sym_Call,
      sym_number,
      anon_sym_DQUOTE,
      anon_sym_True,
      anon_sym_False,
      sym_identifier,
      sym__equal,
      aux_sym__whitespace_token1,
  [3065] = 2,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(232), 27,
      anon_sym_LPAREN,
      anon_sym_COMMA,
      anon_sym_New,
      anon_sym_DOT,
      anon_sym_PLUS,
      anon_sym_DASH,
      anon_sym_STAR,
      anon_sym_SLASH,
      anon_sym_Mod,
      anon_sym_AMP,
      aux_sym_binary_expression_token1,
      aux_sym_binary_expression_token2,
      aux_sym_binary_expression_token3,
      anon_sym_LT_GT,
      anon_sym_LT,
      anon_sym_GT,
      anon_sym_LT_EQ,
      anon_sym_GT_EQ,
      anon_sym_Not,
      anon_sym_Call,
      sym_number,
      anon_sym_DQUOTE,
      anon_sym_True,
      anon_sym_False,
      sym_identifier,
      sym__equal,
      aux_sym__whitespace_token1,
  [3099] = 2,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(234), 27,
      anon_sym_LPAREN,
      anon_sym_COMMA,
      anon_sym_New,
      anon_sym_DOT,
      anon_sym_PLUS,
      anon_sym_DASH,
      anon_sym_STAR,
      anon_sym_SLASH,
      anon_sym_Mod,
      anon_sym_AMP,
      aux_sym_binary_expression_token1,
      aux_sym_binary_expression_token2,
      aux_sym_binary_expression_token3,
      anon_sym_LT_GT,
      anon_sym_LT,
      anon_sym_GT,
      anon_sym_LT_EQ,
      anon_sym_GT_EQ,
      anon_sym_Not,
      anon_sym_Call,
      sym_number,
      anon_sym_DQUOTE,
      anon_sym_True,
      anon_sym_False,
      sym_identifier,
      sym__equal,
      aux_sym__whitespace_token1,
  [3133] = 2,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(236), 27,
      anon_sym_LPAREN,
      anon_sym_COMMA,
      anon_sym_New,
      anon_sym_DOT,
      anon_sym_PLUS,
      anon_sym_DASH,
      anon_sym_STAR,
      anon_sym_SLASH,
      anon_sym_Mod,
      anon_sym_AMP,
      aux_sym_binary_expression_token1,
      aux_sym_binary_expression_token2,
      aux_sym_binary_expression_token3,
      anon_sym_LT_GT,
      anon_sym_LT,
      anon_sym_GT,
      anon_sym_LT_EQ,
      anon_sym_GT_EQ,
      anon_sym_Not,
      anon_sym_Call,
      sym_number,
      anon_sym_DQUOTE,
      anon_sym_True,
      anon_sym_False,
      sym_identifier,
      sym__equal,
      aux_sym__whitespace_token1,
  [3167] = 2,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(234), 27,
      anon_sym_LPAREN,
      anon_sym_COMMA,
      anon_sym_New,
      anon_sym_DOT,
      anon_sym_PLUS,
      anon_sym_DASH,
      anon_sym_STAR,
      anon_sym_SLASH,
      anon_sym_Mod,
      anon_sym_AMP,
      aux_sym_binary_expression_token1,
      aux_sym_binary_expression_token2,
      aux_sym_binary_expression_token3,
      anon_sym_LT_GT,
      anon_sym_LT,
      anon_sym_GT,
      anon_sym_LT_EQ,
      anon_sym_GT_EQ,
      anon_sym_Not,
      anon_sym_Call,
      sym_number,
      anon_sym_DQUOTE,
      anon_sym_True,
      anon_sym_False,
      sym_identifier,
      sym__equal,
      aux_sym__whitespace_token1,
  [3201] = 2,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(238), 27,
      anon_sym_LPAREN,
      anon_sym_COMMA,
      anon_sym_New,
      anon_sym_DOT,
      anon_sym_PLUS,
      anon_sym_DASH,
      anon_sym_STAR,
      anon_sym_SLASH,
      anon_sym_Mod,
      anon_sym_AMP,
      aux_sym_binary_expression_token1,
      aux_sym_binary_expression_token2,
      aux_sym_binary_expression_token3,
      anon_sym_LT_GT,
      anon_sym_LT,
      anon_sym_GT,
      anon_sym_LT_EQ,
      anon_sym_GT_EQ,
      anon_sym_Not,
      anon_sym_Call,
      sym_number,
      anon_sym_DQUOTE,
      anon_sym_True,
      anon_sym_False,
      sym_identifier,
      sym__equal,
      aux_sym__whitespace_token1,
  [3235] = 6,
    ACTIONS(192), 1,
      anon_sym_DOT,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(194), 2,
      anon_sym_PLUS,
      anon_sym_DASH,
    ACTIONS(198), 3,
      anon_sym_STAR,
      anon_sym_SLASH,
      anon_sym_Mod,
    ACTIONS(200), 10,
      anon_sym_AMP,
      aux_sym_binary_expression_token1,
      aux_sym_binary_expression_token2,
      aux_sym_binary_expression_token3,
      anon_sym_LT_GT,
      anon_sym_LT,
      anon_sym_GT,
      anon_sym_LT_EQ,
      anon_sym_GT_EQ,
      sym__equal,
    ACTIONS(226), 11,
      anon_sym_LPAREN,
      anon_sym_COMMA,
      anon_sym_New,
      anon_sym_Not,
      anon_sym_Call,
      sym_number,
      anon_sym_DQUOTE,
      anon_sym_True,
      anon_sym_False,
      sym_identifier,
      aux_sym__whitespace_token1,
  [3277] = 3,
    ACTIONS(228), 1,
      anon_sym_LPAREN,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(236), 26,
      anon_sym_COMMA,
      anon_sym_New,
      anon_sym_DOT,
      anon_sym_PLUS,
      anon_sym_DASH,
      anon_sym_STAR,
      anon_sym_SLASH,
      anon_sym_Mod,
      anon_sym_AMP,
      aux_sym_binary_expression_token1,
      aux_sym_binary_expression_token2,
      aux_sym_binary_expression_token3,
      anon_sym_LT_GT,
      anon_sym_LT,
      anon_sym_GT,
      anon_sym_LT_EQ,
      anon_sym_GT_EQ,
      anon_sym_Not,
      anon_sym_Call,
      sym_number,
      anon_sym_DQUOTE,
      anon_sym_True,
      anon_sym_False,
      sym_identifier,
      sym__equal,
      aux_sym__whitespace_token1,
  [3313] = 2,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(240), 27,
      anon_sym_LPAREN,
      anon_sym_COMMA,
      anon_sym_New,
      anon_sym_DOT,
      anon_sym_PLUS,
      anon_sym_DASH,
      anon_sym_STAR,
      anon_sym_SLASH,
      anon_sym_Mod,
      anon_sym_AMP,
      aux_sym_binary_expression_token1,
      aux_sym_binary_expression_token2,
      aux_sym_binary_expression_token3,
      anon_sym_LT_GT,
      anon_sym_LT,
      anon_sym_GT,
      anon_sym_LT_EQ,
      anon_sym_GT_EQ,
      anon_sym_Not,
      anon_sym_Call,
      sym_number,
      anon_sym_DQUOTE,
      anon_sym_True,
      anon_sym_False,
      sym_identifier,
      sym__equal,
      aux_sym__whitespace_token1,
  [3347] = 2,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(242), 27,
      anon_sym_LPAREN,
      anon_sym_COMMA,
      anon_sym_New,
      anon_sym_DOT,
      anon_sym_PLUS,
      anon_sym_DASH,
      anon_sym_STAR,
      anon_sym_SLASH,
      anon_sym_Mod,
      anon_sym_AMP,
      aux_sym_binary_expression_token1,
      aux_sym_binary_expression_token2,
      aux_sym_binary_expression_token3,
      anon_sym_LT_GT,
      anon_sym_LT,
      anon_sym_GT,
      anon_sym_LT_EQ,
      anon_sym_GT_EQ,
      anon_sym_Not,
      anon_sym_Call,
      sym_number,
      anon_sym_DQUOTE,
      anon_sym_True,
      anon_sym_False,
      sym_identifier,
      sym__equal,
      aux_sym__whitespace_token1,
  [3381] = 4,
    ACTIONS(244), 1,
      anon_sym_LPAREN,
    ACTIONS(246), 1,
      sym__equal,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(230), 24,
      anon_sym_New,
      anon_sym_DOT,
      anon_sym_PLUS,
      anon_sym_DASH,
      anon_sym_STAR,
      anon_sym_SLASH,
      anon_sym_Mod,
      anon_sym_AMP,
      aux_sym_binary_expression_token1,
      aux_sym_binary_expression_token2,
      aux_sym_binary_expression_token3,
      anon_sym_LT_GT,
      anon_sym_LT,
      anon_sym_GT,
      anon_sym_LT_EQ,
      anon_sym_GT_EQ,
      anon_sym_Not,
      anon_sym_Call,
      sym_number,
      anon_sym_DQUOTE,
      anon_sym_True,
      anon_sym_False,
      sym_identifier,
      aux_sym__whitespace_token1,
  [3418] = 15,
    ACTIONS(117), 1,
      anon_sym_LPAREN,
    ACTIONS(121), 1,
      anon_sym_New,
    ACTIONS(125), 1,
      anon_sym_Call,
    ACTIONS(127), 1,
      sym_number,
    ACTIONS(129), 1,
      anon_sym_DQUOTE,
    ACTIONS(248), 1,
      anon_sym_RPAREN,
    ACTIONS(250), 1,
      sym_identifier,
    STATE(216), 1,
      sym_argument,
    STATE(224), 1,
      sym_keyword_argument,
    STATE(308), 1,
      sym_argument_list,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(123), 2,
      anon_sym_DASH,
      anon_sym_Not,
    ACTIONS(131), 2,
      anon_sym_True,
      anon_sym_False,
    STATE(92), 2,
      sym_string_literal,
      sym_boolean,
    STATE(155), 7,
      sym__expression,
      sym_new_expression,
      sym_member_expression,
      sym_binary_expression,
      sym_unary_expression,
      sym_function_call,
      sym_literal,
  [3474] = 15,
    ACTIONS(117), 1,
      anon_sym_LPAREN,
    ACTIONS(121), 1,
      anon_sym_New,
    ACTIONS(125), 1,
      anon_sym_Call,
    ACTIONS(127), 1,
      sym_number,
    ACTIONS(129), 1,
      anon_sym_DQUOTE,
    ACTIONS(250), 1,
      sym_identifier,
    ACTIONS(252), 1,
      anon_sym_RPAREN,
    STATE(216), 1,
      sym_argument,
    STATE(224), 1,
      sym_keyword_argument,
    STATE(287), 1,
      sym_argument_list,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(123), 2,
      anon_sym_DASH,
      anon_sym_Not,
    ACTIONS(131), 2,
      anon_sym_True,
      anon_sym_False,
    STATE(92), 2,
      sym_string_literal,
      sym_boolean,
    STATE(153), 7,
      sym__expression,
      sym_new_expression,
      sym_member_expression,
      sym_binary_expression,
      sym_unary_expression,
      sym_function_call,
      sym_literal,
  [3530] = 5,
    ACTIONS(45), 1,
      ts_builtin_sym_end,
    ACTIONS(256), 1,
      aux_sym__whitespace_token1,
    STATE(58), 1,
      aux_sym__whitespace,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(254), 20,
      anon_sym_Exit,
      anon_sym_For,
      anon_sym_Function,
      anon_sym_Sub,
      anon_sym_Do,
      anon_sym_While,
      anon_sym_If,
      anon_sym_LPAREN,
      anon_sym_Private,
      anon_sym_Dim,
      anon_sym_ReDim,
      anon_sym_New,
      anon_sym_DASH,
      anon_sym_Not,
      anon_sym_Call,
      sym_number,
      anon_sym_DQUOTE,
      anon_sym_True,
      anon_sym_False,
      sym_identifier,
  [3566] = 15,
    ACTIONS(117), 1,
      anon_sym_LPAREN,
    ACTIONS(121), 1,
      anon_sym_New,
    ACTIONS(125), 1,
      anon_sym_Call,
    ACTIONS(127), 1,
      sym_number,
    ACTIONS(129), 1,
      anon_sym_DQUOTE,
    ACTIONS(250), 1,
      sym_identifier,
    ACTIONS(252), 1,
      anon_sym_RPAREN,
    STATE(216), 1,
      sym_argument,
    STATE(224), 1,
      sym_keyword_argument,
    STATE(287), 1,
      sym_argument_list,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(123), 2,
      anon_sym_DASH,
      anon_sym_Not,
    ACTIONS(131), 2,
      anon_sym_True,
      anon_sym_False,
    STATE(92), 2,
      sym_string_literal,
      sym_boolean,
    STATE(155), 7,
      sym__expression,
      sym_new_expression,
      sym_member_expression,
      sym_binary_expression,
      sym_unary_expression,
      sym_function_call,
      sym_literal,
  [3622] = 15,
    ACTIONS(117), 1,
      anon_sym_LPAREN,
    ACTIONS(121), 1,
      anon_sym_New,
    ACTIONS(125), 1,
      anon_sym_Call,
    ACTIONS(127), 1,
      sym_number,
    ACTIONS(129), 1,
      anon_sym_DQUOTE,
    ACTIONS(250), 1,
      sym_identifier,
    ACTIONS(258), 1,
      anon_sym_RPAREN,
    STATE(216), 1,
      sym_argument,
    STATE(224), 1,
      sym_keyword_argument,
    STATE(276), 1,
      sym_argument_list,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(123), 2,
      anon_sym_DASH,
      anon_sym_Not,
    ACTIONS(131), 2,
      anon_sym_True,
      anon_sym_False,
    STATE(92), 2,
      sym_string_literal,
      sym_boolean,
    STATE(155), 7,
      sym__expression,
      sym_new_expression,
      sym_member_expression,
      sym_binary_expression,
      sym_unary_expression,
      sym_function_call,
      sym_literal,
  [3678] = 15,
    ACTIONS(117), 1,
      anon_sym_LPAREN,
    ACTIONS(121), 1,
      anon_sym_New,
    ACTIONS(125), 1,
      anon_sym_Call,
    ACTIONS(127), 1,
      sym_number,
    ACTIONS(129), 1,
      anon_sym_DQUOTE,
    ACTIONS(250), 1,
      sym_identifier,
    ACTIONS(260), 1,
      anon_sym_RPAREN,
    STATE(216), 1,
      sym_argument,
    STATE(224), 1,
      sym_keyword_argument,
    STATE(319), 1,
      sym_argument_list,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(123), 2,
      anon_sym_DASH,
      anon_sym_Not,
    ACTIONS(131), 2,
      anon_sym_True,
      anon_sym_False,
    STATE(92), 2,
      sym_string_literal,
      sym_boolean,
    STATE(155), 7,
      sym__expression,
      sym_new_expression,
      sym_member_expression,
      sym_binary_expression,
      sym_unary_expression,
      sym_function_call,
      sym_literal,
  [3734] = 5,
    ACTIONS(262), 1,
      ts_builtin_sym_end,
    ACTIONS(266), 1,
      aux_sym__whitespace_token1,
    STATE(58), 1,
      aux_sym__whitespace,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(264), 20,
      anon_sym_Exit,
      anon_sym_For,
      anon_sym_Function,
      anon_sym_Sub,
      anon_sym_Do,
      anon_sym_While,
      anon_sym_If,
      anon_sym_LPAREN,
      anon_sym_Private,
      anon_sym_Dim,
      anon_sym_ReDim,
      anon_sym_New,
      anon_sym_DASH,
      anon_sym_Not,
      anon_sym_Call,
      sym_number,
      anon_sym_DQUOTE,
      anon_sym_True,
      anon_sym_False,
      sym_identifier,
  [3770] = 15,
    ACTIONS(117), 1,
      anon_sym_LPAREN,
    ACTIONS(121), 1,
      anon_sym_New,
    ACTIONS(125), 1,
      anon_sym_Call,
    ACTIONS(127), 1,
      sym_number,
    ACTIONS(129), 1,
      anon_sym_DQUOTE,
    ACTIONS(250), 1,
      sym_identifier,
    ACTIONS(269), 1,
      anon_sym_RPAREN,
    STATE(216), 1,
      sym_argument,
    STATE(224), 1,
      sym_keyword_argument,
    STATE(318), 1,
      sym_argument_list,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(123), 2,
      anon_sym_DASH,
      anon_sym_Not,
    ACTIONS(131), 2,
      anon_sym_True,
      anon_sym_False,
    STATE(92), 2,
      sym_string_literal,
      sym_boolean,
    STATE(155), 7,
      sym__expression,
      sym_new_expression,
      sym_member_expression,
      sym_binary_expression,
      sym_unary_expression,
      sym_function_call,
      sym_literal,
  [3826] = 15,
    ACTIONS(117), 1,
      anon_sym_LPAREN,
    ACTIONS(121), 1,
      anon_sym_New,
    ACTIONS(125), 1,
      anon_sym_Call,
    ACTIONS(127), 1,
      sym_number,
    ACTIONS(129), 1,
      anon_sym_DQUOTE,
    ACTIONS(250), 1,
      sym_identifier,
    ACTIONS(271), 1,
      anon_sym_RPAREN,
    STATE(216), 1,
      sym_argument,
    STATE(224), 1,
      sym_keyword_argument,
    STATE(307), 1,
      sym_argument_list,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(123), 2,
      anon_sym_DASH,
      anon_sym_Not,
    ACTIONS(131), 2,
      anon_sym_True,
      anon_sym_False,
    STATE(92), 2,
      sym_string_literal,
      sym_boolean,
    STATE(155), 7,
      sym__expression,
      sym_new_expression,
      sym_member_expression,
      sym_binary_expression,
      sym_unary_expression,
      sym_function_call,
      sym_literal,
  [3882] = 3,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(220), 2,
      anon_sym_LT,
      anon_sym_GT,
    ACTIONS(273), 20,
      anon_sym_Then,
      anon_sym_To,
      anon_sym_LPAREN,
      anon_sym_RPAREN,
      anon_sym_COMMA,
      anon_sym_Alias,
      anon_sym_DOT,
      anon_sym_PLUS,
      anon_sym_DASH,
      anon_sym_STAR,
      anon_sym_SLASH,
      anon_sym_Mod,
      anon_sym_AMP,
      aux_sym_binary_expression_token1,
      aux_sym_binary_expression_token2,
      aux_sym_binary_expression_token3,
      anon_sym_LT_GT,
      anon_sym_LT_EQ,
      anon_sym_GT_EQ,
      sym__equal,
  [3913] = 4,
    ACTIONS(277), 1,
      anon_sym_LPAREN,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(236), 2,
      anon_sym_LT,
      anon_sym_GT,
    ACTIONS(275), 18,
      anon_sym_Then,
      anon_sym_To,
      anon_sym_RPAREN,
      anon_sym_COMMA,
      anon_sym_DOT,
      anon_sym_PLUS,
      anon_sym_DASH,
      anon_sym_STAR,
      anon_sym_SLASH,
      anon_sym_Mod,
      anon_sym_AMP,
      aux_sym_binary_expression_token1,
      aux_sym_binary_expression_token2,
      aux_sym_binary_expression_token3,
      anon_sym_LT_GT,
      anon_sym_LT_EQ,
      anon_sym_GT_EQ,
      sym__equal,
  [3945] = 13,
    ACTIONS(188), 1,
      anon_sym_LPAREN,
    ACTIONS(190), 1,
      anon_sym_New,
    ACTIONS(204), 1,
      anon_sym_Call,
    ACTIONS(206), 1,
      sym_number,
    ACTIONS(208), 1,
      anon_sym_DQUOTE,
    ACTIONS(212), 1,
      sym_identifier,
    STATE(240), 1,
      sym_keyword_argument,
    STATE(255), 1,
      sym_argument,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(202), 2,
      anon_sym_DASH,
      anon_sym_Not,
    ACTIONS(210), 2,
      anon_sym_True,
      anon_sym_False,
    STATE(140), 2,
      sym_string_literal,
      sym_boolean,
    STATE(156), 7,
      sym__expression,
      sym_new_expression,
      sym_member_expression,
      sym_binary_expression,
      sym_unary_expression,
      sym_function_call,
      sym_literal,
  [3995] = 4,
    ACTIONS(277), 1,
      anon_sym_LPAREN,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(230), 2,
      anon_sym_LT,
      anon_sym_GT,
    ACTIONS(279), 18,
      anon_sym_Then,
      anon_sym_To,
      anon_sym_RPAREN,
      anon_sym_COMMA,
      anon_sym_DOT,
      anon_sym_PLUS,
      anon_sym_DASH,
      anon_sym_STAR,
      anon_sym_SLASH,
      anon_sym_Mod,
      anon_sym_AMP,
      aux_sym_binary_expression_token1,
      aux_sym_binary_expression_token2,
      aux_sym_binary_expression_token3,
      anon_sym_LT_GT,
      anon_sym_LT_EQ,
      anon_sym_GT_EQ,
      sym__equal,
  [4027] = 4,
    ACTIONS(281), 1,
      aux_sym__whitespace_token1,
    STATE(66), 1,
      aux_sym__whitespace,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(156), 19,
      anon_sym_Exit,
      anon_sym_For,
      anon_sym_Do,
      anon_sym_While,
      anon_sym_If,
      anon_sym_Else,
      anon_sym_EndIf,
      anon_sym_LPAREN,
      anon_sym_Dim,
      anon_sym_ReDim,
      anon_sym_New,
      anon_sym_DASH,
      anon_sym_Not,
      anon_sym_Call,
      sym_number,
      anon_sym_DQUOTE,
      anon_sym_True,
      anon_sym_False,
      sym_identifier,
  [4059] = 4,
    ACTIONS(283), 1,
      aux_sym__whitespace_token1,
    STATE(66), 1,
      aux_sym__whitespace,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(264), 19,
      anon_sym_Exit,
      anon_sym_For,
      anon_sym_Do,
      anon_sym_While,
      anon_sym_If,
      anon_sym_Else,
      anon_sym_EndIf,
      anon_sym_LPAREN,
      anon_sym_Dim,
      anon_sym_ReDim,
      anon_sym_New,
      anon_sym_DASH,
      anon_sym_Not,
      anon_sym_Call,
      sym_number,
      anon_sym_DQUOTE,
      anon_sym_True,
      anon_sym_False,
      sym_identifier,
  [4091] = 13,
    ACTIONS(117), 1,
      anon_sym_LPAREN,
    ACTIONS(121), 1,
      anon_sym_New,
    ACTIONS(125), 1,
      anon_sym_Call,
    ACTIONS(127), 1,
      sym_number,
    ACTIONS(129), 1,
      anon_sym_DQUOTE,
    ACTIONS(250), 1,
      sym_identifier,
    STATE(224), 1,
      sym_keyword_argument,
    STATE(247), 1,
      sym_argument,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(123), 2,
      anon_sym_DASH,
      anon_sym_Not,
    ACTIONS(131), 2,
      anon_sym_True,
      anon_sym_False,
    STATE(92), 2,
      sym_string_literal,
      sym_boolean,
    STATE(155), 7,
      sym__expression,
      sym_new_expression,
      sym_member_expression,
      sym_binary_expression,
      sym_unary_expression,
      sym_function_call,
      sym_literal,
  [4141] = 4,
    ACTIONS(277), 1,
      anon_sym_LPAREN,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(234), 2,
      anon_sym_LT,
      anon_sym_GT,
    ACTIONS(286), 18,
      anon_sym_Then,
      anon_sym_To,
      anon_sym_RPAREN,
      anon_sym_COMMA,
      anon_sym_DOT,
      anon_sym_PLUS,
      anon_sym_DASH,
      anon_sym_STAR,
      anon_sym_SLASH,
      anon_sym_Mod,
      anon_sym_AMP,
      aux_sym_binary_expression_token1,
      aux_sym_binary_expression_token2,
      aux_sym_binary_expression_token3,
      anon_sym_LT_GT,
      anon_sym_LT_EQ,
      anon_sym_GT_EQ,
      sym__equal,
  [4173] = 5,
    ACTIONS(283), 1,
      aux_sym__whitespace_token1,
    STATE(66), 1,
      aux_sym__whitespace,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(139), 2,
      anon_sym_Else,
      anon_sym_EndIf,
    ACTIONS(264), 17,
      anon_sym_Exit,
      anon_sym_For,
      anon_sym_Do,
      anon_sym_While,
      anon_sym_If,
      anon_sym_LPAREN,
      anon_sym_Dim,
      anon_sym_ReDim,
      anon_sym_New,
      anon_sym_DASH,
      anon_sym_Not,
      anon_sym_Call,
      sym_number,
      anon_sym_DQUOTE,
      anon_sym_True,
      anon_sym_False,
      sym_identifier,
  [4207] = 3,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(232), 2,
      anon_sym_LT,
      anon_sym_GT,
    ACTIONS(288), 19,
      anon_sym_Then,
      anon_sym_To,
      anon_sym_RPAREN,
      anon_sym_COMMA,
      anon_sym_LPAREN_RPAREN,
      anon_sym_DOT,
      anon_sym_PLUS,
      anon_sym_DASH,
      anon_sym_STAR,
      anon_sym_SLASH,
      anon_sym_Mod,
      anon_sym_AMP,
      aux_sym_binary_expression_token1,
      aux_sym_binary_expression_token2,
      aux_sym_binary_expression_token3,
      anon_sym_LT_GT,
      anon_sym_LT_EQ,
      anon_sym_GT_EQ,
      sym__equal,
  [4237] = 4,
    ACTIONS(290), 1,
      aux_sym__whitespace_token1,
    STATE(75), 1,
      aux_sym__whitespace,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(156), 18,
      anon_sym_Exit,
      anon_sym_For,
      anon_sym_Do,
      anon_sym_While,
      anon_sym_If,
      anon_sym_EndIf,
      anon_sym_LPAREN,
      anon_sym_Dim,
      anon_sym_ReDim,
      anon_sym_New,
      anon_sym_DASH,
      anon_sym_Not,
      anon_sym_Call,
      sym_number,
      anon_sym_DQUOTE,
      anon_sym_True,
      anon_sym_False,
      sym_identifier,
  [4268] = 5,
    ACTIONS(139), 1,
      anon_sym_Wend,
    ACTIONS(292), 1,
      aux_sym__whitespace_token1,
    STATE(74), 1,
      aux_sym__whitespace,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(264), 17,
      anon_sym_Exit,
      anon_sym_For,
      anon_sym_Do,
      anon_sym_While,
      anon_sym_If,
      anon_sym_LPAREN,
      anon_sym_Dim,
      anon_sym_ReDim,
      anon_sym_New,
      anon_sym_DASH,
      anon_sym_Not,
      anon_sym_Call,
      sym_number,
      anon_sym_DQUOTE,
      anon_sym_True,
      anon_sym_False,
      sym_identifier,
  [4301] = 5,
    ACTIONS(139), 1,
      anon_sym_EndIf,
    ACTIONS(295), 1,
      aux_sym__whitespace_token1,
    STATE(75), 1,
      aux_sym__whitespace,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(264), 17,
      anon_sym_Exit,
      anon_sym_For,
      anon_sym_Do,
      anon_sym_While,
      anon_sym_If,
      anon_sym_LPAREN,
      anon_sym_Dim,
      anon_sym_ReDim,
      anon_sym_New,
      anon_sym_DASH,
      anon_sym_Not,
      anon_sym_Call,
      sym_number,
      anon_sym_DQUOTE,
      anon_sym_True,
      anon_sym_False,
      sym_identifier,
  [4334] = 4,
    ACTIONS(292), 1,
      aux_sym__whitespace_token1,
    STATE(74), 1,
      aux_sym__whitespace,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(264), 18,
      anon_sym_Exit,
      anon_sym_For,
      anon_sym_Do,
      anon_sym_While,
      anon_sym_If,
      anon_sym_Wend,
      anon_sym_LPAREN,
      anon_sym_Dim,
      anon_sym_ReDim,
      anon_sym_New,
      anon_sym_DASH,
      anon_sym_Not,
      anon_sym_Call,
      sym_number,
      anon_sym_DQUOTE,
      anon_sym_True,
      anon_sym_False,
      sym_identifier,
  [4365] = 4,
    ACTIONS(295), 1,
      aux_sym__whitespace_token1,
    STATE(75), 1,
      aux_sym__whitespace,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(264), 18,
      anon_sym_Exit,
      anon_sym_For,
      anon_sym_Do,
      anon_sym_While,
      anon_sym_If,
      anon_sym_EndIf,
      anon_sym_LPAREN,
      anon_sym_Dim,
      anon_sym_ReDim,
      anon_sym_New,
      anon_sym_DASH,
      anon_sym_Not,
      anon_sym_Call,
      sym_number,
      anon_sym_DQUOTE,
      anon_sym_True,
      anon_sym_False,
      sym_identifier,
  [4396] = 4,
    ACTIONS(298), 1,
      aux_sym__whitespace_token1,
    STATE(97), 1,
      aux_sym__whitespace,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(156), 18,
      anon_sym_Exit,
      anon_sym_For,
      anon_sym_Do,
      anon_sym_While,
      anon_sym_If,
      anon_sym_Next,
      anon_sym_LPAREN,
      anon_sym_Dim,
      anon_sym_ReDim,
      anon_sym_New,
      anon_sym_DASH,
      anon_sym_Not,
      anon_sym_Call,
      sym_number,
      anon_sym_DQUOTE,
      anon_sym_True,
      anon_sym_False,
      sym_identifier,
  [4427] = 13,
    ACTIONS(117), 1,
      anon_sym_LPAREN,
    ACTIONS(121), 1,
      anon_sym_New,
    ACTIONS(125), 1,
      anon_sym_Call,
    ACTIONS(127), 1,
      sym_number,
    ACTIONS(129), 1,
      anon_sym_DQUOTE,
    ACTIONS(300), 1,
      sym_identifier,
    STATE(133), 1,
      sym_type_member_expression,
    STATE(173), 1,
      sym_member_expression,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(123), 2,
      anon_sym_DASH,
      anon_sym_Not,
    ACTIONS(131), 2,
      anon_sym_True,
      anon_sym_False,
    STATE(92), 2,
      sym_string_literal,
      sym_boolean,
    STATE(174), 6,
      sym__expression,
      sym_new_expression,
      sym_binary_expression,
      sym_unary_expression,
      sym_function_call,
      sym_literal,
  [4476] = 3,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(224), 2,
      anon_sym_LT,
      anon_sym_GT,
    ACTIONS(302), 18,
      anon_sym_Then,
      anon_sym_To,
      anon_sym_RPAREN,
      anon_sym_COMMA,
      anon_sym_DOT,
      anon_sym_PLUS,
      anon_sym_DASH,
      anon_sym_STAR,
      anon_sym_SLASH,
      anon_sym_Mod,
      anon_sym_AMP,
      aux_sym_binary_expression_token1,
      aux_sym_binary_expression_token2,
      aux_sym_binary_expression_token3,
      anon_sym_LT_GT,
      anon_sym_LT_EQ,
      anon_sym_GT_EQ,
      sym__equal,
  [4505] = 6,
    ACTIONS(306), 1,
      anon_sym_DOT,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(222), 2,
      anon_sym_LT,
      anon_sym_GT,
    ACTIONS(308), 2,
      anon_sym_PLUS,
      anon_sym_DASH,
    ACTIONS(310), 3,
      anon_sym_STAR,
      anon_sym_SLASH,
      anon_sym_Mod,
    ACTIONS(304), 12,
      anon_sym_Then,
      anon_sym_To,
      anon_sym_RPAREN,
      anon_sym_COMMA,
      anon_sym_AMP,
      aux_sym_binary_expression_token1,
      aux_sym_binary_expression_token2,
      aux_sym_binary_expression_token3,
      anon_sym_LT_GT,
      anon_sym_LT_EQ,
      anon_sym_GT_EQ,
      sym__equal,
  [4540] = 4,
    ACTIONS(312), 1,
      aux_sym__whitespace_token1,
    STATE(80), 1,
      aux_sym__whitespace,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(264), 18,
      anon_sym_Exit,
      anon_sym_For,
      anon_sym_Do,
      anon_sym_While,
      anon_sym_If,
      anon_sym_LPAREN,
      anon_sym_EndSub,
      anon_sym_Dim,
      anon_sym_ReDim,
      anon_sym_New,
      anon_sym_DASH,
      anon_sym_Not,
      anon_sym_Call,
      sym_number,
      anon_sym_DQUOTE,
      anon_sym_True,
      anon_sym_False,
      sym_identifier,
  [4571] = 3,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(216), 2,
      anon_sym_LT,
      anon_sym_GT,
    ACTIONS(315), 18,
      anon_sym_Then,
      anon_sym_To,
      anon_sym_RPAREN,
      anon_sym_COMMA,
      anon_sym_DOT,
      anon_sym_PLUS,
      anon_sym_DASH,
      anon_sym_STAR,
      anon_sym_SLASH,
      anon_sym_Mod,
      anon_sym_AMP,
      aux_sym_binary_expression_token1,
      aux_sym_binary_expression_token2,
      aux_sym_binary_expression_token3,
      anon_sym_LT_GT,
      anon_sym_LT_EQ,
      anon_sym_GT_EQ,
      sym__equal,
  [4600] = 3,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(240), 2,
      anon_sym_LT,
      anon_sym_GT,
    ACTIONS(317), 18,
      anon_sym_Then,
      anon_sym_To,
      anon_sym_RPAREN,
      anon_sym_COMMA,
      anon_sym_DOT,
      anon_sym_PLUS,
      anon_sym_DASH,
      anon_sym_STAR,
      anon_sym_SLASH,
      anon_sym_Mod,
      anon_sym_AMP,
      aux_sym_binary_expression_token1,
      aux_sym_binary_expression_token2,
      aux_sym_binary_expression_token3,
      anon_sym_LT_GT,
      anon_sym_LT_EQ,
      anon_sym_GT_EQ,
      sym__equal,
  [4629] = 3,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(242), 2,
      anon_sym_LT,
      anon_sym_GT,
    ACTIONS(319), 18,
      anon_sym_Then,
      anon_sym_To,
      anon_sym_RPAREN,
      anon_sym_COMMA,
      anon_sym_DOT,
      anon_sym_PLUS,
      anon_sym_DASH,
      anon_sym_STAR,
      anon_sym_SLASH,
      anon_sym_Mod,
      anon_sym_AMP,
      aux_sym_binary_expression_token1,
      aux_sym_binary_expression_token2,
      aux_sym_binary_expression_token3,
      anon_sym_LT_GT,
      anon_sym_LT_EQ,
      anon_sym_GT_EQ,
      sym__equal,
  [4658] = 5,
    ACTIONS(139), 1,
      anon_sym_Loop,
    ACTIONS(321), 1,
      aux_sym__whitespace_token1,
    STATE(102), 1,
      aux_sym__whitespace,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(264), 17,
      anon_sym_Exit,
      anon_sym_For,
      anon_sym_Do,
      anon_sym_While,
      anon_sym_If,
      anon_sym_LPAREN,
      anon_sym_Dim,
      anon_sym_ReDim,
      anon_sym_New,
      anon_sym_DASH,
      anon_sym_Not,
      anon_sym_Call,
      sym_number,
      anon_sym_DQUOTE,
      anon_sym_True,
      anon_sym_False,
      sym_identifier,
  [4691] = 7,
    ACTIONS(306), 1,
      anon_sym_DOT,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(308), 2,
      anon_sym_PLUS,
      anon_sym_DASH,
    ACTIONS(328), 2,
      anon_sym_LT,
      anon_sym_GT,
    ACTIONS(310), 3,
      anon_sym_STAR,
      anon_sym_SLASH,
      anon_sym_Mod,
    ACTIONS(324), 4,
      anon_sym_Then,
      anon_sym_To,
      anon_sym_RPAREN,
      anon_sym_COMMA,
    ACTIONS(326), 8,
      anon_sym_AMP,
      aux_sym_binary_expression_token1,
      aux_sym_binary_expression_token2,
      aux_sym_binary_expression_token3,
      anon_sym_LT_GT,
      anon_sym_LT_EQ,
      anon_sym_GT_EQ,
      sym__equal,
  [4728] = 3,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(234), 2,
      anon_sym_LT,
      anon_sym_GT,
    ACTIONS(286), 18,
      anon_sym_Then,
      anon_sym_To,
      anon_sym_RPAREN,
      anon_sym_COMMA,
      anon_sym_DOT,
      anon_sym_PLUS,
      anon_sym_DASH,
      anon_sym_STAR,
      anon_sym_SLASH,
      anon_sym_Mod,
      anon_sym_AMP,
      aux_sym_binary_expression_token1,
      aux_sym_binary_expression_token2,
      aux_sym_binary_expression_token3,
      anon_sym_LT_GT,
      anon_sym_LT_EQ,
      anon_sym_GT_EQ,
      sym__equal,
  [4757] = 3,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(236), 2,
      anon_sym_LT,
      anon_sym_GT,
    ACTIONS(275), 18,
      anon_sym_Then,
      anon_sym_To,
      anon_sym_RPAREN,
      anon_sym_COMMA,
      anon_sym_DOT,
      anon_sym_PLUS,
      anon_sym_DASH,
      anon_sym_STAR,
      anon_sym_SLASH,
      anon_sym_Mod,
      anon_sym_AMP,
      aux_sym_binary_expression_token1,
      aux_sym_binary_expression_token2,
      aux_sym_binary_expression_token3,
      anon_sym_LT_GT,
      anon_sym_LT_EQ,
      anon_sym_GT_EQ,
      sym__equal,
  [4786] = 3,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(218), 2,
      anon_sym_LT,
      anon_sym_GT,
    ACTIONS(330), 18,
      anon_sym_Then,
      anon_sym_To,
      anon_sym_RPAREN,
      anon_sym_COMMA,
      anon_sym_DOT,
      anon_sym_PLUS,
      anon_sym_DASH,
      anon_sym_STAR,
      anon_sym_SLASH,
      anon_sym_Mod,
      anon_sym_AMP,
      aux_sym_binary_expression_token1,
      aux_sym_binary_expression_token2,
      aux_sym_binary_expression_token3,
      anon_sym_LT_GT,
      anon_sym_LT_EQ,
      anon_sym_GT_EQ,
      sym__equal,
  [4815] = 5,
    ACTIONS(139), 1,
      anon_sym_Next,
    ACTIONS(332), 1,
      aux_sym__whitespace_token1,
    STATE(97), 1,
      aux_sym__whitespace,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(264), 17,
      anon_sym_Exit,
      anon_sym_For,
      anon_sym_Do,
      anon_sym_While,
      anon_sym_If,
      anon_sym_LPAREN,
      anon_sym_Dim,
      anon_sym_ReDim,
      anon_sym_New,
      anon_sym_DASH,
      anon_sym_Not,
      anon_sym_Call,
      sym_number,
      anon_sym_DQUOTE,
      anon_sym_True,
      anon_sym_False,
      sym_identifier,
  [4848] = 4,
    ACTIONS(279), 1,
      aux_sym__whitespace_token1,
    ACTIONS(335), 1,
      anon_sym_LPAREN,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(230), 18,
      anon_sym_Step,
      anon_sym_COMMA,
      anon_sym_DOT,
      anon_sym_PLUS,
      anon_sym_DASH,
      anon_sym_STAR,
      anon_sym_SLASH,
      anon_sym_Mod,
      anon_sym_AMP,
      aux_sym_binary_expression_token1,
      aux_sym_binary_expression_token2,
      aux_sym_binary_expression_token3,
      anon_sym_LT_GT,
      anon_sym_LT,
      anon_sym_GT,
      anon_sym_LT_EQ,
      anon_sym_GT_EQ,
      sym__equal,
  [4879] = 13,
    ACTIONS(117), 1,
      anon_sym_LPAREN,
    ACTIONS(121), 1,
      anon_sym_New,
    ACTIONS(125), 1,
      anon_sym_Call,
    ACTIONS(127), 1,
      sym_number,
    ACTIONS(129), 1,
      anon_sym_DQUOTE,
    ACTIONS(337), 1,
      sym_identifier,
    STATE(86), 1,
      sym_type_member_expression,
    STATE(176), 1,
      sym_member_expression,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(123), 2,
      anon_sym_DASH,
      anon_sym_Not,
    ACTIONS(131), 2,
      anon_sym_True,
      anon_sym_False,
    STATE(92), 2,
      sym_string_literal,
      sym_boolean,
    STATE(174), 6,
      sym__expression,
      sym_new_expression,
      sym_binary_expression,
      sym_unary_expression,
      sym_function_call,
      sym_literal,
  [4928] = 3,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(238), 2,
      anon_sym_LT,
      anon_sym_GT,
    ACTIONS(339), 18,
      anon_sym_Then,
      anon_sym_To,
      anon_sym_RPAREN,
      anon_sym_COMMA,
      anon_sym_DOT,
      anon_sym_PLUS,
      anon_sym_DASH,
      anon_sym_STAR,
      anon_sym_SLASH,
      anon_sym_Mod,
      anon_sym_AMP,
      aux_sym_binary_expression_token1,
      aux_sym_binary_expression_token2,
      aux_sym_binary_expression_token3,
      anon_sym_LT_GT,
      anon_sym_LT_EQ,
      anon_sym_GT_EQ,
      sym__equal,
  [4957] = 3,
    ACTIONS(288), 1,
      aux_sym__whitespace_token1,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(232), 19,
      anon_sym_Step,
      anon_sym_COMMA,
      anon_sym_LPAREN_RPAREN,
      anon_sym_DOT,
      anon_sym_PLUS,
      anon_sym_DASH,
      anon_sym_STAR,
      anon_sym_SLASH,
      anon_sym_Mod,
      anon_sym_AMP,
      aux_sym_binary_expression_token1,
      aux_sym_binary_expression_token2,
      aux_sym_binary_expression_token3,
      anon_sym_LT_GT,
      anon_sym_LT,
      anon_sym_GT,
      anon_sym_LT_EQ,
      anon_sym_GT_EQ,
      sym__equal,
  [4986] = 13,
    ACTIONS(117), 1,
      anon_sym_LPAREN,
    ACTIONS(121), 1,
      anon_sym_New,
    ACTIONS(125), 1,
      anon_sym_Call,
    ACTIONS(127), 1,
      sym_number,
    ACTIONS(129), 1,
      anon_sym_DQUOTE,
    ACTIONS(341), 1,
      sym_identifier,
    STATE(45), 1,
      sym_type_member_expression,
    STATE(175), 1,
      sym_member_expression,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(123), 2,
      anon_sym_DASH,
      anon_sym_Not,
    ACTIONS(131), 2,
      anon_sym_True,
      anon_sym_False,
    STATE(92), 2,
      sym_string_literal,
      sym_boolean,
    STATE(174), 6,
      sym__expression,
      sym_new_expression,
      sym_binary_expression,
      sym_unary_expression,
      sym_function_call,
      sym_literal,
  [5035] = 12,
    ACTIONS(188), 1,
      anon_sym_LPAREN,
    ACTIONS(190), 1,
      anon_sym_New,
    ACTIONS(204), 1,
      anon_sym_Call,
    ACTIONS(206), 1,
      sym_number,
    ACTIONS(208), 1,
      anon_sym_DQUOTE,
    ACTIONS(343), 1,
      anon_sym_Preserve,
    ACTIONS(345), 1,
      sym_identifier,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(202), 2,
      anon_sym_DASH,
      anon_sym_Not,
    ACTIONS(210), 2,
      anon_sym_True,
      anon_sym_False,
    STATE(140), 2,
      sym_string_literal,
      sym_boolean,
    STATE(166), 7,
      sym__expression,
      sym_new_expression,
      sym_member_expression,
      sym_binary_expression,
      sym_unary_expression,
      sym_function_call,
      sym_literal,
  [5082] = 4,
    ACTIONS(347), 1,
      aux_sym__whitespace_token1,
    STATE(74), 1,
      aux_sym__whitespace,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(156), 18,
      anon_sym_Exit,
      anon_sym_For,
      anon_sym_Do,
      anon_sym_While,
      anon_sym_If,
      anon_sym_Wend,
      anon_sym_LPAREN,
      anon_sym_Dim,
      anon_sym_ReDim,
      anon_sym_New,
      anon_sym_DASH,
      anon_sym_Not,
      anon_sym_Call,
      sym_number,
      anon_sym_DQUOTE,
      anon_sym_True,
      anon_sym_False,
      sym_identifier,
  [5113] = 4,
    ACTIONS(332), 1,
      aux_sym__whitespace_token1,
    STATE(97), 1,
      aux_sym__whitespace,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(264), 18,
      anon_sym_Exit,
      anon_sym_For,
      anon_sym_Do,
      anon_sym_While,
      anon_sym_If,
      anon_sym_Next,
      anon_sym_LPAREN,
      anon_sym_Dim,
      anon_sym_ReDim,
      anon_sym_New,
      anon_sym_DASH,
      anon_sym_Not,
      anon_sym_Call,
      sym_number,
      anon_sym_DQUOTE,
      anon_sym_True,
      anon_sym_False,
      sym_identifier,
  [5144] = 5,
    ACTIONS(139), 1,
      anon_sym_EndFunction,
    ACTIONS(349), 1,
      aux_sym__whitespace_token1,
    STATE(106), 1,
      aux_sym__whitespace,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(264), 17,
      anon_sym_Exit,
      anon_sym_For,
      anon_sym_Do,
      anon_sym_While,
      anon_sym_If,
      anon_sym_LPAREN,
      anon_sym_Dim,
      anon_sym_ReDim,
      anon_sym_New,
      anon_sym_DASH,
      anon_sym_Not,
      anon_sym_Call,
      sym_number,
      anon_sym_DQUOTE,
      anon_sym_True,
      anon_sym_False,
      sym_identifier,
  [5177] = 5,
    ACTIONS(306), 1,
      anon_sym_DOT,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(222), 2,
      anon_sym_LT,
      anon_sym_GT,
    ACTIONS(310), 3,
      anon_sym_STAR,
      anon_sym_SLASH,
      anon_sym_Mod,
    ACTIONS(304), 14,
      anon_sym_Then,
      anon_sym_To,
      anon_sym_RPAREN,
      anon_sym_COMMA,
      anon_sym_PLUS,
      anon_sym_DASH,
      anon_sym_AMP,
      aux_sym_binary_expression_token1,
      aux_sym_binary_expression_token2,
      aux_sym_binary_expression_token3,
      anon_sym_LT_GT,
      anon_sym_LT_EQ,
      anon_sym_GT_EQ,
      sym__equal,
  [5210] = 4,
    ACTIONS(275), 1,
      aux_sym__whitespace_token1,
    ACTIONS(335), 1,
      anon_sym_LPAREN,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(236), 18,
      anon_sym_Step,
      anon_sym_COMMA,
      anon_sym_DOT,
      anon_sym_PLUS,
      anon_sym_DASH,
      anon_sym_STAR,
      anon_sym_SLASH,
      anon_sym_Mod,
      anon_sym_AMP,
      aux_sym_binary_expression_token1,
      aux_sym_binary_expression_token2,
      aux_sym_binary_expression_token3,
      anon_sym_LT_GT,
      anon_sym_LT,
      anon_sym_GT,
      anon_sym_LT_EQ,
      anon_sym_GT_EQ,
      sym__equal,
  [5241] = 4,
    ACTIONS(306), 1,
      anon_sym_DOT,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(222), 2,
      anon_sym_LT,
      anon_sym_GT,
    ACTIONS(304), 17,
      anon_sym_Then,
      anon_sym_To,
      anon_sym_RPAREN,
      anon_sym_COMMA,
      anon_sym_PLUS,
      anon_sym_DASH,
      anon_sym_STAR,
      anon_sym_SLASH,
      anon_sym_Mod,
      anon_sym_AMP,
      aux_sym_binary_expression_token1,
      aux_sym_binary_expression_token2,
      aux_sym_binary_expression_token3,
      anon_sym_LT_GT,
      anon_sym_LT_EQ,
      anon_sym_GT_EQ,
      sym__equal,
  [5272] = 4,
    ACTIONS(321), 1,
      aux_sym__whitespace_token1,
    STATE(102), 1,
      aux_sym__whitespace,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(264), 18,
      anon_sym_Exit,
      anon_sym_For,
      anon_sym_Do,
      anon_sym_While,
      anon_sym_If,
      anon_sym_Loop,
      anon_sym_LPAREN,
      anon_sym_Dim,
      anon_sym_ReDim,
      anon_sym_New,
      anon_sym_DASH,
      anon_sym_Not,
      anon_sym_Call,
      sym_number,
      anon_sym_DQUOTE,
      anon_sym_True,
      anon_sym_False,
      sym_identifier,
  [5303] = 5,
    ACTIONS(279), 1,
      aux_sym__whitespace_token1,
    ACTIONS(335), 1,
      anon_sym_LPAREN,
    ACTIONS(352), 1,
      anon_sym_COLON_EQ,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(230), 17,
      anon_sym_COMMA,
      anon_sym_DOT,
      anon_sym_PLUS,
      anon_sym_DASH,
      anon_sym_STAR,
      anon_sym_SLASH,
      anon_sym_Mod,
      anon_sym_AMP,
      aux_sym_binary_expression_token1,
      aux_sym_binary_expression_token2,
      aux_sym_binary_expression_token3,
      anon_sym_LT_GT,
      anon_sym_LT,
      anon_sym_GT,
      anon_sym_LT_EQ,
      anon_sym_GT_EQ,
      sym__equal,
  [5336] = 4,
    ACTIONS(354), 1,
      aux_sym__whitespace_token1,
    STATE(80), 1,
      aux_sym__whitespace,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(156), 18,
      anon_sym_Exit,
      anon_sym_For,
      anon_sym_Do,
      anon_sym_While,
      anon_sym_If,
      anon_sym_LPAREN,
      anon_sym_EndSub,
      anon_sym_Dim,
      anon_sym_ReDim,
      anon_sym_New,
      anon_sym_DASH,
      anon_sym_Not,
      anon_sym_Call,
      sym_number,
      anon_sym_DQUOTE,
      anon_sym_True,
      anon_sym_False,
      sym_identifier,
  [5367] = 4,
    ACTIONS(356), 1,
      aux_sym__whitespace_token1,
    STATE(106), 1,
      aux_sym__whitespace,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(156), 18,
      anon_sym_Exit,
      anon_sym_For,
      anon_sym_Do,
      anon_sym_While,
      anon_sym_If,
      anon_sym_LPAREN,
      anon_sym_EndFunction,
      anon_sym_Dim,
      anon_sym_ReDim,
      anon_sym_New,
      anon_sym_DASH,
      anon_sym_Not,
      anon_sym_Call,
      sym_number,
      anon_sym_DQUOTE,
      anon_sym_True,
      anon_sym_False,
      sym_identifier,
  [5398] = 4,
    ACTIONS(349), 1,
      aux_sym__whitespace_token1,
    STATE(106), 1,
      aux_sym__whitespace,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(264), 18,
      anon_sym_Exit,
      anon_sym_For,
      anon_sym_Do,
      anon_sym_While,
      anon_sym_If,
      anon_sym_LPAREN,
      anon_sym_EndFunction,
      anon_sym_Dim,
      anon_sym_ReDim,
      anon_sym_New,
      anon_sym_DASH,
      anon_sym_Not,
      anon_sym_Call,
      sym_number,
      anon_sym_DQUOTE,
      anon_sym_True,
      anon_sym_False,
      sym_identifier,
  [5429] = 4,
    ACTIONS(286), 1,
      aux_sym__whitespace_token1,
    ACTIONS(358), 1,
      anon_sym_LPAREN,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(234), 18,
      anon_sym_Step,
      anon_sym_COMMA,
      anon_sym_DOT,
      anon_sym_PLUS,
      anon_sym_DASH,
      anon_sym_STAR,
      anon_sym_SLASH,
      anon_sym_Mod,
      anon_sym_AMP,
      aux_sym_binary_expression_token1,
      aux_sym_binary_expression_token2,
      aux_sym_binary_expression_token3,
      anon_sym_LT_GT,
      anon_sym_LT,
      anon_sym_GT,
      anon_sym_LT_EQ,
      anon_sym_GT_EQ,
      sym__equal,
  [5460] = 5,
    ACTIONS(277), 1,
      anon_sym_LPAREN,
    ACTIONS(360), 1,
      anon_sym_COLON_EQ,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(230), 2,
      anon_sym_LT,
      anon_sym_GT,
    ACTIONS(279), 16,
      anon_sym_RPAREN,
      anon_sym_COMMA,
      anon_sym_DOT,
      anon_sym_PLUS,
      anon_sym_DASH,
      anon_sym_STAR,
      anon_sym_SLASH,
      anon_sym_Mod,
      anon_sym_AMP,
      aux_sym_binary_expression_token1,
      aux_sym_binary_expression_token2,
      aux_sym_binary_expression_token3,
      anon_sym_LT_GT,
      anon_sym_LT_EQ,
      anon_sym_GT_EQ,
      sym__equal,
  [5493] = 5,
    ACTIONS(139), 1,
      anon_sym_EndSub,
    ACTIONS(312), 1,
      aux_sym__whitespace_token1,
    STATE(80), 1,
      aux_sym__whitespace,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(264), 17,
      anon_sym_Exit,
      anon_sym_For,
      anon_sym_Do,
      anon_sym_While,
      anon_sym_If,
      anon_sym_LPAREN,
      anon_sym_Dim,
      anon_sym_ReDim,
      anon_sym_New,
      anon_sym_DASH,
      anon_sym_Not,
      anon_sym_Call,
      sym_number,
      anon_sym_DQUOTE,
      anon_sym_True,
      anon_sym_False,
      sym_identifier,
  [5526] = 4,
    ACTIONS(362), 1,
      aux_sym__whitespace_token1,
    STATE(102), 1,
      aux_sym__whitespace,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(156), 18,
      anon_sym_Exit,
      anon_sym_For,
      anon_sym_Do,
      anon_sym_While,
      anon_sym_If,
      anon_sym_Loop,
      anon_sym_LPAREN,
      anon_sym_Dim,
      anon_sym_ReDim,
      anon_sym_New,
      anon_sym_DASH,
      anon_sym_Not,
      anon_sym_Call,
      sym_number,
      anon_sym_DQUOTE,
      anon_sym_True,
      anon_sym_False,
      sym_identifier,
  [5557] = 11,
    ACTIONS(23), 1,
      anon_sym_LPAREN,
    ACTIONS(31), 1,
      anon_sym_New,
    ACTIONS(35), 1,
      anon_sym_Call,
    ACTIONS(37), 1,
      sym_number,
    ACTIONS(39), 1,
      anon_sym_DQUOTE,
    ACTIONS(364), 1,
      sym_identifier,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(33), 2,
      anon_sym_DASH,
      anon_sym_Not,
    ACTIONS(41), 2,
      anon_sym_True,
      anon_sym_False,
    STATE(46), 2,
      sym_string_literal,
      sym_boolean,
    STATE(39), 7,
      sym__expression,
      sym_new_expression,
      sym_member_expression,
      sym_binary_expression,
      sym_unary_expression,
      sym_function_call,
      sym_literal,
  [5601] = 11,
    ACTIONS(117), 1,
      anon_sym_LPAREN,
    ACTIONS(121), 1,
      anon_sym_New,
    ACTIONS(125), 1,
      anon_sym_Call,
    ACTIONS(127), 1,
      sym_number,
    ACTIONS(129), 1,
      anon_sym_DQUOTE,
    ACTIONS(366), 1,
      sym_identifier,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(123), 2,
      anon_sym_DASH,
      anon_sym_Not,
    ACTIONS(131), 2,
      anon_sym_True,
      anon_sym_False,
    STATE(92), 2,
      sym_string_literal,
      sym_boolean,
    STATE(101), 7,
      sym__expression,
      sym_new_expression,
      sym_member_expression,
      sym_binary_expression,
      sym_unary_expression,
      sym_function_call,
      sym_literal,
  [5645] = 3,
    ACTIONS(273), 1,
      aux_sym__whitespace_token1,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(220), 18,
      anon_sym_Step,
      anon_sym_COMMA,
      anon_sym_DOT,
      anon_sym_PLUS,
      anon_sym_DASH,
      anon_sym_STAR,
      anon_sym_SLASH,
      anon_sym_Mod,
      anon_sym_AMP,
      aux_sym_binary_expression_token1,
      aux_sym_binary_expression_token2,
      aux_sym_binary_expression_token3,
      anon_sym_LT_GT,
      anon_sym_LT,
      anon_sym_GT,
      anon_sym_LT_EQ,
      anon_sym_GT_EQ,
      sym__equal,
  [5673] = 3,
    ACTIONS(317), 1,
      aux_sym__whitespace_token1,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(240), 18,
      anon_sym_Step,
      anon_sym_COMMA,
      anon_sym_DOT,
      anon_sym_PLUS,
      anon_sym_DASH,
      anon_sym_STAR,
      anon_sym_SLASH,
      anon_sym_Mod,
      anon_sym_AMP,
      aux_sym_binary_expression_token1,
      aux_sym_binary_expression_token2,
      aux_sym_binary_expression_token3,
      anon_sym_LT_GT,
      anon_sym_LT,
      anon_sym_GT,
      anon_sym_LT_EQ,
      anon_sym_GT_EQ,
      sym__equal,
  [5701] = 11,
    ACTIONS(117), 1,
      anon_sym_LPAREN,
    ACTIONS(121), 1,
      anon_sym_New,
    ACTIONS(125), 1,
      anon_sym_Call,
    ACTIONS(127), 1,
      sym_number,
    ACTIONS(129), 1,
      anon_sym_DQUOTE,
    ACTIONS(366), 1,
      sym_identifier,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(123), 2,
      anon_sym_DASH,
      anon_sym_Not,
    ACTIONS(131), 2,
      anon_sym_True,
      anon_sym_False,
    STATE(92), 2,
      sym_string_literal,
      sym_boolean,
    STATE(170), 7,
      sym__expression,
      sym_new_expression,
      sym_member_expression,
      sym_binary_expression,
      sym_unary_expression,
      sym_function_call,
      sym_literal,
  [5745] = 11,
    ACTIONS(23), 1,
      anon_sym_LPAREN,
    ACTIONS(31), 1,
      anon_sym_New,
    ACTIONS(35), 1,
      anon_sym_Call,
    ACTIONS(37), 1,
      sym_number,
    ACTIONS(39), 1,
      anon_sym_DQUOTE,
    ACTIONS(364), 1,
      sym_identifier,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(33), 2,
      anon_sym_DASH,
      anon_sym_Not,
    ACTIONS(41), 2,
      anon_sym_True,
      anon_sym_False,
    STATE(46), 2,
      sym_string_literal,
      sym_boolean,
    STATE(47), 7,
      sym__expression,
      sym_new_expression,
      sym_member_expression,
      sym_binary_expression,
      sym_unary_expression,
      sym_function_call,
      sym_literal,
  [5789] = 11,
    ACTIONS(117), 1,
      anon_sym_LPAREN,
    ACTIONS(121), 1,
      anon_sym_New,
    ACTIONS(125), 1,
      anon_sym_Call,
    ACTIONS(127), 1,
      sym_number,
    ACTIONS(129), 1,
      anon_sym_DQUOTE,
    ACTIONS(366), 1,
      sym_identifier,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(123), 2,
      anon_sym_DASH,
      anon_sym_Not,
    ACTIONS(131), 2,
      anon_sym_True,
      anon_sym_False,
    STATE(92), 2,
      sym_string_literal,
      sym_boolean,
    STATE(163), 7,
      sym__expression,
      sym_new_expression,
      sym_member_expression,
      sym_binary_expression,
      sym_unary_expression,
      sym_function_call,
      sym_literal,
  [5833] = 11,
    ACTIONS(117), 1,
      anon_sym_LPAREN,
    ACTIONS(121), 1,
      anon_sym_New,
    ACTIONS(125), 1,
      anon_sym_Call,
    ACTIONS(127), 1,
      sym_number,
    ACTIONS(129), 1,
      anon_sym_DQUOTE,
    ACTIONS(366), 1,
      sym_identifier,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(123), 2,
      anon_sym_DASH,
      anon_sym_Not,
    ACTIONS(131), 2,
      anon_sym_True,
      anon_sym_False,
    STATE(92), 2,
      sym_string_literal,
      sym_boolean,
    STATE(85), 7,
      sym__expression,
      sym_new_expression,
      sym_member_expression,
      sym_binary_expression,
      sym_unary_expression,
      sym_function_call,
      sym_literal,
  [5877] = 3,
    ACTIONS(315), 1,
      aux_sym__whitespace_token1,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(216), 18,
      anon_sym_Step,
      anon_sym_COMMA,
      anon_sym_DOT,
      anon_sym_PLUS,
      anon_sym_DASH,
      anon_sym_STAR,
      anon_sym_SLASH,
      anon_sym_Mod,
      anon_sym_AMP,
      aux_sym_binary_expression_token1,
      aux_sym_binary_expression_token2,
      aux_sym_binary_expression_token3,
      anon_sym_LT_GT,
      anon_sym_LT,
      anon_sym_GT,
      anon_sym_LT_EQ,
      anon_sym_GT_EQ,
      sym__equal,
  [5905] = 3,
    ACTIONS(319), 1,
      aux_sym__whitespace_token1,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(242), 18,
      anon_sym_Step,
      anon_sym_COMMA,
      anon_sym_DOT,
      anon_sym_PLUS,
      anon_sym_DASH,
      anon_sym_STAR,
      anon_sym_SLASH,
      anon_sym_Mod,
      anon_sym_AMP,
      aux_sym_binary_expression_token1,
      aux_sym_binary_expression_token2,
      aux_sym_binary_expression_token3,
      anon_sym_LT_GT,
      anon_sym_LT,
      anon_sym_GT,
      anon_sym_LT_EQ,
      anon_sym_GT_EQ,
      sym__equal,
  [5933] = 11,
    ACTIONS(188), 1,
      anon_sym_LPAREN,
    ACTIONS(190), 1,
      anon_sym_New,
    ACTIONS(204), 1,
      anon_sym_Call,
    ACTIONS(206), 1,
      sym_number,
    ACTIONS(208), 1,
      anon_sym_DQUOTE,
    ACTIONS(345), 1,
      sym_identifier,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(202), 2,
      anon_sym_DASH,
      anon_sym_Not,
    ACTIONS(210), 2,
      anon_sym_True,
      anon_sym_False,
    STATE(140), 2,
      sym_string_literal,
      sym_boolean,
    STATE(131), 7,
      sym__expression,
      sym_new_expression,
      sym_member_expression,
      sym_binary_expression,
      sym_unary_expression,
      sym_function_call,
      sym_literal,
  [5977] = 11,
    ACTIONS(188), 1,
      anon_sym_LPAREN,
    ACTIONS(190), 1,
      anon_sym_New,
    ACTIONS(204), 1,
      anon_sym_Call,
    ACTIONS(206), 1,
      sym_number,
    ACTIONS(208), 1,
      anon_sym_DQUOTE,
    ACTIONS(345), 1,
      sym_identifier,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(202), 2,
      anon_sym_DASH,
      anon_sym_Not,
    ACTIONS(210), 2,
      anon_sym_True,
      anon_sym_False,
    STATE(140), 2,
      sym_string_literal,
      sym_boolean,
    STATE(127), 7,
      sym__expression,
      sym_new_expression,
      sym_member_expression,
      sym_binary_expression,
      sym_unary_expression,
      sym_function_call,
      sym_literal,
  [6021] = 11,
    ACTIONS(188), 1,
      anon_sym_LPAREN,
    ACTIONS(190), 1,
      anon_sym_New,
    ACTIONS(204), 1,
      anon_sym_Call,
    ACTIONS(206), 1,
      sym_number,
    ACTIONS(208), 1,
      anon_sym_DQUOTE,
    ACTIONS(345), 1,
      sym_identifier,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(202), 2,
      anon_sym_DASH,
      anon_sym_Not,
    ACTIONS(210), 2,
      anon_sym_True,
      anon_sym_False,
    STATE(140), 2,
      sym_string_literal,
      sym_boolean,
    STATE(168), 7,
      sym__expression,
      sym_new_expression,
      sym_member_expression,
      sym_binary_expression,
      sym_unary_expression,
      sym_function_call,
      sym_literal,
  [6065] = 11,
    ACTIONS(188), 1,
      anon_sym_LPAREN,
    ACTIONS(190), 1,
      anon_sym_New,
    ACTIONS(204), 1,
      anon_sym_Call,
    ACTIONS(206), 1,
      sym_number,
    ACTIONS(208), 1,
      anon_sym_DQUOTE,
    ACTIONS(345), 1,
      sym_identifier,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(202), 2,
      anon_sym_DASH,
      anon_sym_Not,
    ACTIONS(210), 2,
      anon_sym_True,
      anon_sym_False,
    STATE(140), 2,
      sym_string_literal,
      sym_boolean,
    STATE(158), 7,
      sym__expression,
      sym_new_expression,
      sym_member_expression,
      sym_binary_expression,
      sym_unary_expression,
      sym_function_call,
      sym_literal,
  [6109] = 11,
    ACTIONS(188), 1,
      anon_sym_LPAREN,
    ACTIONS(190), 1,
      anon_sym_New,
    ACTIONS(204), 1,
      anon_sym_Call,
    ACTIONS(206), 1,
      sym_number,
    ACTIONS(208), 1,
      anon_sym_DQUOTE,
    ACTIONS(345), 1,
      sym_identifier,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(202), 2,
      anon_sym_DASH,
      anon_sym_Not,
    ACTIONS(210), 2,
      anon_sym_True,
      anon_sym_False,
    STATE(140), 2,
      sym_string_literal,
      sym_boolean,
    STATE(139), 7,
      sym__expression,
      sym_new_expression,
      sym_member_expression,
      sym_binary_expression,
      sym_unary_expression,
      sym_function_call,
      sym_literal,
  [6153] = 11,
    ACTIONS(188), 1,
      anon_sym_LPAREN,
    ACTIONS(190), 1,
      anon_sym_New,
    ACTIONS(204), 1,
      anon_sym_Call,
    ACTIONS(206), 1,
      sym_number,
    ACTIONS(208), 1,
      anon_sym_DQUOTE,
    ACTIONS(345), 1,
      sym_identifier,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(202), 2,
      anon_sym_DASH,
      anon_sym_Not,
    ACTIONS(210), 2,
      anon_sym_True,
      anon_sym_False,
    STATE(140), 2,
      sym_string_literal,
      sym_boolean,
    STATE(143), 7,
      sym__expression,
      sym_new_expression,
      sym_member_expression,
      sym_binary_expression,
      sym_unary_expression,
      sym_function_call,
      sym_literal,
  [6197] = 7,
    ACTIONS(324), 1,
      aux_sym__whitespace_token1,
    ACTIONS(368), 1,
      anon_sym_DOT,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(226), 2,
      anon_sym_Step,
      anon_sym_COMMA,
    ACTIONS(370), 2,
      anon_sym_PLUS,
      anon_sym_DASH,
    ACTIONS(372), 3,
      anon_sym_STAR,
      anon_sym_SLASH,
      anon_sym_Mod,
    ACTIONS(374), 10,
      anon_sym_AMP,
      aux_sym_binary_expression_token1,
      aux_sym_binary_expression_token2,
      aux_sym_binary_expression_token3,
      anon_sym_LT_GT,
      anon_sym_LT,
      anon_sym_GT,
      anon_sym_LT_EQ,
      anon_sym_GT_EQ,
      sym__equal,
  [6233] = 11,
    ACTIONS(188), 1,
      anon_sym_LPAREN,
    ACTIONS(190), 1,
      anon_sym_New,
    ACTIONS(204), 1,
      anon_sym_Call,
    ACTIONS(206), 1,
      sym_number,
    ACTIONS(208), 1,
      anon_sym_DQUOTE,
    ACTIONS(345), 1,
      sym_identifier,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(202), 2,
      anon_sym_DASH,
      anon_sym_Not,
    ACTIONS(210), 2,
      anon_sym_True,
      anon_sym_False,
    STATE(140), 2,
      sym_string_literal,
      sym_boolean,
    STATE(154), 7,
      sym__expression,
      sym_new_expression,
      sym_member_expression,
      sym_binary_expression,
      sym_unary_expression,
      sym_function_call,
      sym_literal,
  [6277] = 11,
    ACTIONS(117), 1,
      anon_sym_LPAREN,
    ACTIONS(121), 1,
      anon_sym_New,
    ACTIONS(125), 1,
      anon_sym_Call,
    ACTIONS(127), 1,
      sym_number,
    ACTIONS(129), 1,
      anon_sym_DQUOTE,
    ACTIONS(366), 1,
      sym_identifier,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(123), 2,
      anon_sym_DASH,
      anon_sym_Not,
    ACTIONS(131), 2,
      anon_sym_True,
      anon_sym_False,
    STATE(92), 2,
      sym_string_literal,
      sym_boolean,
    STATE(99), 7,
      sym__expression,
      sym_new_expression,
      sym_member_expression,
      sym_binary_expression,
      sym_unary_expression,
      sym_function_call,
      sym_literal,
  [6321] = 11,
    ACTIONS(117), 1,
      anon_sym_LPAREN,
    ACTIONS(121), 1,
      anon_sym_New,
    ACTIONS(125), 1,
      anon_sym_Call,
    ACTIONS(127), 1,
      sym_number,
    ACTIONS(129), 1,
      anon_sym_DQUOTE,
    ACTIONS(366), 1,
      sym_identifier,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(123), 2,
      anon_sym_DASH,
      anon_sym_Not,
    ACTIONS(131), 2,
      anon_sym_True,
      anon_sym_False,
    STATE(92), 2,
      sym_string_literal,
      sym_boolean,
    STATE(157), 7,
      sym__expression,
      sym_new_expression,
      sym_member_expression,
      sym_binary_expression,
      sym_unary_expression,
      sym_function_call,
      sym_literal,
  [6365] = 6,
    ACTIONS(304), 1,
      aux_sym__whitespace_token1,
    ACTIONS(368), 1,
      anon_sym_DOT,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(370), 2,
      anon_sym_PLUS,
      anon_sym_DASH,
    ACTIONS(372), 3,
      anon_sym_STAR,
      anon_sym_SLASH,
      anon_sym_Mod,
    ACTIONS(222), 12,
      anon_sym_Step,
      anon_sym_COMMA,
      anon_sym_AMP,
      aux_sym_binary_expression_token1,
      aux_sym_binary_expression_token2,
      aux_sym_binary_expression_token3,
      anon_sym_LT_GT,
      anon_sym_LT,
      anon_sym_GT,
      anon_sym_LT_EQ,
      anon_sym_GT_EQ,
      sym__equal,
  [6399] = 8,
    ACTIONS(368), 1,
      anon_sym_DOT,
    ACTIONS(376), 1,
      anon_sym_Step,
    ACTIONS(378), 1,
      aux_sym__whitespace_token1,
    STATE(4), 1,
      aux_sym__whitespace,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(370), 2,
      anon_sym_PLUS,
      anon_sym_DASH,
    ACTIONS(372), 3,
      anon_sym_STAR,
      anon_sym_SLASH,
      anon_sym_Mod,
    ACTIONS(374), 10,
      anon_sym_AMP,
      aux_sym_binary_expression_token1,
      aux_sym_binary_expression_token2,
      aux_sym_binary_expression_token3,
      anon_sym_LT_GT,
      anon_sym_LT,
      anon_sym_GT,
      anon_sym_LT_EQ,
      anon_sym_GT_EQ,
      sym__equal,
  [6437] = 3,
    ACTIONS(286), 1,
      aux_sym__whitespace_token1,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(234), 18,
      anon_sym_Step,
      anon_sym_COMMA,
      anon_sym_DOT,
      anon_sym_PLUS,
      anon_sym_DASH,
      anon_sym_STAR,
      anon_sym_SLASH,
      anon_sym_Mod,
      anon_sym_AMP,
      aux_sym_binary_expression_token1,
      aux_sym_binary_expression_token2,
      aux_sym_binary_expression_token3,
      anon_sym_LT_GT,
      anon_sym_LT,
      anon_sym_GT,
      anon_sym_LT_EQ,
      anon_sym_GT_EQ,
      sym__equal,
  [6465] = 11,
    ACTIONS(117), 1,
      anon_sym_LPAREN,
    ACTIONS(121), 1,
      anon_sym_New,
    ACTIONS(125), 1,
      anon_sym_Call,
    ACTIONS(127), 1,
      sym_number,
    ACTIONS(129), 1,
      anon_sym_DQUOTE,
    ACTIONS(366), 1,
      sym_identifier,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(123), 2,
      anon_sym_DASH,
      anon_sym_Not,
    ACTIONS(131), 2,
      anon_sym_True,
      anon_sym_False,
    STATE(92), 2,
      sym_string_literal,
      sym_boolean,
    STATE(79), 7,
      sym__expression,
      sym_new_expression,
      sym_member_expression,
      sym_binary_expression,
      sym_unary_expression,
      sym_function_call,
      sym_literal,
  [6509] = 11,
    ACTIONS(188), 1,
      anon_sym_LPAREN,
    ACTIONS(190), 1,
      anon_sym_New,
    ACTIONS(204), 1,
      anon_sym_Call,
    ACTIONS(206), 1,
      sym_number,
    ACTIONS(208), 1,
      anon_sym_DQUOTE,
    ACTIONS(345), 1,
      sym_identifier,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(202), 2,
      anon_sym_DASH,
      anon_sym_Not,
    ACTIONS(210), 2,
      anon_sym_True,
      anon_sym_False,
    STATE(140), 2,
      sym_string_literal,
      sym_boolean,
    STATE(132), 7,
      sym__expression,
      sym_new_expression,
      sym_member_expression,
      sym_binary_expression,
      sym_unary_expression,
      sym_function_call,
      sym_literal,
  [6553] = 11,
    ACTIONS(188), 1,
      anon_sym_LPAREN,
    ACTIONS(190), 1,
      anon_sym_New,
    ACTIONS(204), 1,
      anon_sym_Call,
    ACTIONS(206), 1,
      sym_number,
    ACTIONS(208), 1,
      anon_sym_DQUOTE,
    ACTIONS(345), 1,
      sym_identifier,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(202), 2,
      anon_sym_DASH,
      anon_sym_Not,
    ACTIONS(210), 2,
      anon_sym_True,
      anon_sym_False,
    STATE(140), 2,
      sym_string_literal,
      sym_boolean,
    STATE(161), 7,
      sym__expression,
      sym_new_expression,
      sym_member_expression,
      sym_binary_expression,
      sym_unary_expression,
      sym_function_call,
      sym_literal,
  [6597] = 11,
    ACTIONS(188), 1,
      anon_sym_LPAREN,
    ACTIONS(190), 1,
      anon_sym_New,
    ACTIONS(204), 1,
      anon_sym_Call,
    ACTIONS(206), 1,
      sym_number,
    ACTIONS(208), 1,
      anon_sym_DQUOTE,
    ACTIONS(345), 1,
      sym_identifier,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(202), 2,
      anon_sym_DASH,
      anon_sym_Not,
    ACTIONS(210), 2,
      anon_sym_True,
      anon_sym_False,
    STATE(140), 2,
      sym_string_literal,
      sym_boolean,
    STATE(159), 7,
      sym__expression,
      sym_new_expression,
      sym_member_expression,
      sym_binary_expression,
      sym_unary_expression,
      sym_function_call,
      sym_literal,
  [6641] = 3,
    ACTIONS(330), 1,
      aux_sym__whitespace_token1,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(218), 18,
      anon_sym_Step,
      anon_sym_COMMA,
      anon_sym_DOT,
      anon_sym_PLUS,
      anon_sym_DASH,
      anon_sym_STAR,
      anon_sym_SLASH,
      anon_sym_Mod,
      anon_sym_AMP,
      aux_sym_binary_expression_token1,
      aux_sym_binary_expression_token2,
      aux_sym_binary_expression_token3,
      anon_sym_LT_GT,
      anon_sym_LT,
      anon_sym_GT,
      anon_sym_LT_EQ,
      anon_sym_GT_EQ,
      sym__equal,
  [6669] = 4,
    ACTIONS(304), 1,
      aux_sym__whitespace_token1,
    ACTIONS(368), 1,
      anon_sym_DOT,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(222), 17,
      anon_sym_Step,
      anon_sym_COMMA,
      anon_sym_PLUS,
      anon_sym_DASH,
      anon_sym_STAR,
      anon_sym_SLASH,
      anon_sym_Mod,
      anon_sym_AMP,
      aux_sym_binary_expression_token1,
      aux_sym_binary_expression_token2,
      aux_sym_binary_expression_token3,
      anon_sym_LT_GT,
      anon_sym_LT,
      anon_sym_GT,
      anon_sym_LT_EQ,
      anon_sym_GT_EQ,
      sym__equal,
  [6699] = 3,
    ACTIONS(339), 1,
      aux_sym__whitespace_token1,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(238), 18,
      anon_sym_Step,
      anon_sym_COMMA,
      anon_sym_DOT,
      anon_sym_PLUS,
      anon_sym_DASH,
      anon_sym_STAR,
      anon_sym_SLASH,
      anon_sym_Mod,
      anon_sym_AMP,
      aux_sym_binary_expression_token1,
      aux_sym_binary_expression_token2,
      aux_sym_binary_expression_token3,
      anon_sym_LT_GT,
      anon_sym_LT,
      anon_sym_GT,
      anon_sym_LT_EQ,
      anon_sym_GT_EQ,
      sym__equal,
  [6727] = 11,
    ACTIONS(117), 1,
      anon_sym_LPAREN,
    ACTIONS(121), 1,
      anon_sym_New,
    ACTIONS(125), 1,
      anon_sym_Call,
    ACTIONS(127), 1,
      sym_number,
    ACTIONS(129), 1,
      anon_sym_DQUOTE,
    ACTIONS(366), 1,
      sym_identifier,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(123), 2,
      anon_sym_DASH,
      anon_sym_Not,
    ACTIONS(131), 2,
      anon_sym_True,
      anon_sym_False,
    STATE(92), 2,
      sym_string_literal,
      sym_boolean,
    STATE(172), 7,
      sym__expression,
      sym_new_expression,
      sym_member_expression,
      sym_binary_expression,
      sym_unary_expression,
      sym_function_call,
      sym_literal,
  [6771] = 11,
    ACTIONS(188), 1,
      anon_sym_LPAREN,
    ACTIONS(190), 1,
      anon_sym_New,
    ACTIONS(204), 1,
      anon_sym_Call,
    ACTIONS(206), 1,
      sym_number,
    ACTIONS(208), 1,
      anon_sym_DQUOTE,
    ACTIONS(345), 1,
      sym_identifier,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(202), 2,
      anon_sym_DASH,
      anon_sym_Not,
    ACTIONS(210), 2,
      anon_sym_True,
      anon_sym_False,
    STATE(140), 2,
      sym_string_literal,
      sym_boolean,
    STATE(167), 7,
      sym__expression,
      sym_new_expression,
      sym_member_expression,
      sym_binary_expression,
      sym_unary_expression,
      sym_function_call,
      sym_literal,
  [6815] = 5,
    ACTIONS(304), 1,
      aux_sym__whitespace_token1,
    ACTIONS(368), 1,
      anon_sym_DOT,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(372), 3,
      anon_sym_STAR,
      anon_sym_SLASH,
      anon_sym_Mod,
    ACTIONS(222), 14,
      anon_sym_Step,
      anon_sym_COMMA,
      anon_sym_PLUS,
      anon_sym_DASH,
      anon_sym_AMP,
      aux_sym_binary_expression_token1,
      aux_sym_binary_expression_token2,
      aux_sym_binary_expression_token3,
      anon_sym_LT_GT,
      anon_sym_LT,
      anon_sym_GT,
      anon_sym_LT_EQ,
      anon_sym_GT_EQ,
      sym__equal,
  [6847] = 11,
    ACTIONS(117), 1,
      anon_sym_LPAREN,
    ACTIONS(121), 1,
      anon_sym_New,
    ACTIONS(125), 1,
      anon_sym_Call,
    ACTIONS(127), 1,
      sym_number,
    ACTIONS(129), 1,
      anon_sym_DQUOTE,
    ACTIONS(366), 1,
      sym_identifier,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(123), 2,
      anon_sym_DASH,
      anon_sym_Not,
    ACTIONS(131), 2,
      anon_sym_True,
      anon_sym_False,
    STATE(92), 2,
      sym_string_literal,
      sym_boolean,
    STATE(169), 7,
      sym__expression,
      sym_new_expression,
      sym_member_expression,
      sym_binary_expression,
      sym_unary_expression,
      sym_function_call,
      sym_literal,
  [6891] = 11,
    ACTIONS(188), 1,
      anon_sym_LPAREN,
    ACTIONS(190), 1,
      anon_sym_New,
    ACTIONS(204), 1,
      anon_sym_Call,
    ACTIONS(206), 1,
      sym_number,
    ACTIONS(208), 1,
      anon_sym_DQUOTE,
    ACTIONS(345), 1,
      sym_identifier,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(202), 2,
      anon_sym_DASH,
      anon_sym_Not,
    ACTIONS(210), 2,
      anon_sym_True,
      anon_sym_False,
    STATE(140), 2,
      sym_string_literal,
      sym_boolean,
    STATE(164), 7,
      sym__expression,
      sym_new_expression,
      sym_member_expression,
      sym_binary_expression,
      sym_unary_expression,
      sym_function_call,
      sym_literal,
  [6935] = 11,
    ACTIONS(188), 1,
      anon_sym_LPAREN,
    ACTIONS(190), 1,
      anon_sym_New,
    ACTIONS(204), 1,
      anon_sym_Call,
    ACTIONS(206), 1,
      sym_number,
    ACTIONS(208), 1,
      anon_sym_DQUOTE,
    ACTIONS(345), 1,
      sym_identifier,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(202), 2,
      anon_sym_DASH,
      anon_sym_Not,
    ACTIONS(210), 2,
      anon_sym_True,
      anon_sym_False,
    STATE(140), 2,
      sym_string_literal,
      sym_boolean,
    STATE(160), 7,
      sym__expression,
      sym_new_expression,
      sym_member_expression,
      sym_binary_expression,
      sym_unary_expression,
      sym_function_call,
      sym_literal,
  [6979] = 3,
    ACTIONS(275), 1,
      aux_sym__whitespace_token1,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(236), 18,
      anon_sym_Step,
      anon_sym_COMMA,
      anon_sym_DOT,
      anon_sym_PLUS,
      anon_sym_DASH,
      anon_sym_STAR,
      anon_sym_SLASH,
      anon_sym_Mod,
      anon_sym_AMP,
      aux_sym_binary_expression_token1,
      aux_sym_binary_expression_token2,
      aux_sym_binary_expression_token3,
      anon_sym_LT_GT,
      anon_sym_LT,
      anon_sym_GT,
      anon_sym_LT_EQ,
      anon_sym_GT_EQ,
      sym__equal,
  [7007] = 11,
    ACTIONS(23), 1,
      anon_sym_LPAREN,
    ACTIONS(31), 1,
      anon_sym_New,
    ACTIONS(35), 1,
      anon_sym_Call,
    ACTIONS(37), 1,
      sym_number,
    ACTIONS(39), 1,
      anon_sym_DQUOTE,
    ACTIONS(364), 1,
      sym_identifier,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(33), 2,
      anon_sym_DASH,
      anon_sym_Not,
    ACTIONS(41), 2,
      anon_sym_True,
      anon_sym_False,
    STATE(46), 2,
      sym_string_literal,
      sym_boolean,
    STATE(37), 7,
      sym__expression,
      sym_new_expression,
      sym_member_expression,
      sym_binary_expression,
      sym_unary_expression,
      sym_function_call,
      sym_literal,
  [7051] = 11,
    ACTIONS(23), 1,
      anon_sym_LPAREN,
    ACTIONS(31), 1,
      anon_sym_New,
    ACTIONS(35), 1,
      anon_sym_Call,
    ACTIONS(37), 1,
      sym_number,
    ACTIONS(39), 1,
      anon_sym_DQUOTE,
    ACTIONS(364), 1,
      sym_identifier,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(33), 2,
      anon_sym_DASH,
      anon_sym_Not,
    ACTIONS(41), 2,
      anon_sym_True,
      anon_sym_False,
    STATE(46), 2,
      sym_string_literal,
      sym_boolean,
    STATE(36), 7,
      sym__expression,
      sym_new_expression,
      sym_member_expression,
      sym_binary_expression,
      sym_unary_expression,
      sym_function_call,
      sym_literal,
  [7095] = 11,
    ACTIONS(117), 1,
      anon_sym_LPAREN,
    ACTIONS(121), 1,
      anon_sym_New,
    ACTIONS(125), 1,
      anon_sym_Call,
    ACTIONS(127), 1,
      sym_number,
    ACTIONS(129), 1,
      anon_sym_DQUOTE,
    ACTIONS(366), 1,
      sym_identifier,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(123), 2,
      anon_sym_DASH,
      anon_sym_Not,
    ACTIONS(131), 2,
      anon_sym_True,
      anon_sym_False,
    STATE(92), 2,
      sym_string_literal,
      sym_boolean,
    STATE(171), 7,
      sym__expression,
      sym_new_expression,
      sym_member_expression,
      sym_binary_expression,
      sym_unary_expression,
      sym_function_call,
      sym_literal,
  [7139] = 3,
    ACTIONS(302), 1,
      aux_sym__whitespace_token1,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(224), 18,
      anon_sym_Step,
      anon_sym_COMMA,
      anon_sym_DOT,
      anon_sym_PLUS,
      anon_sym_DASH,
      anon_sym_STAR,
      anon_sym_SLASH,
      anon_sym_Mod,
      anon_sym_AMP,
      aux_sym_binary_expression_token1,
      aux_sym_binary_expression_token2,
      aux_sym_binary_expression_token3,
      anon_sym_LT_GT,
      anon_sym_LT,
      anon_sym_GT,
      anon_sym_LT_EQ,
      anon_sym_GT_EQ,
      sym__equal,
  [7167] = 11,
    ACTIONS(23), 1,
      anon_sym_LPAREN,
    ACTIONS(31), 1,
      anon_sym_New,
    ACTIONS(35), 1,
      anon_sym_Call,
    ACTIONS(37), 1,
      sym_number,
    ACTIONS(39), 1,
      anon_sym_DQUOTE,
    ACTIONS(364), 1,
      sym_identifier,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(33), 2,
      anon_sym_DASH,
      anon_sym_Not,
    ACTIONS(41), 2,
      anon_sym_True,
      anon_sym_False,
    STATE(46), 2,
      sym_string_literal,
      sym_boolean,
    STATE(40), 7,
      sym__expression,
      sym_new_expression,
      sym_member_expression,
      sym_binary_expression,
      sym_unary_expression,
      sym_function_call,
      sym_literal,
  [7211] = 8,
    ACTIONS(306), 1,
      anon_sym_DOT,
    ACTIONS(380), 1,
      anon_sym_RPAREN,
    ACTIONS(382), 1,
      anon_sym_COMMA,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(308), 2,
      anon_sym_PLUS,
      anon_sym_DASH,
    ACTIONS(328), 2,
      anon_sym_LT,
      anon_sym_GT,
    ACTIONS(310), 3,
      anon_sym_STAR,
      anon_sym_SLASH,
      anon_sym_Mod,
    ACTIONS(326), 8,
      anon_sym_AMP,
      aux_sym_binary_expression_token1,
      aux_sym_binary_expression_token2,
      aux_sym_binary_expression_token3,
      anon_sym_LT_GT,
      anon_sym_LT_EQ,
      anon_sym_GT_EQ,
      sym__equal,
  [7248] = 7,
    ACTIONS(368), 1,
      anon_sym_DOT,
    ACTIONS(384), 1,
      anon_sym_COMMA,
    ACTIONS(386), 1,
      aux_sym__whitespace_token1,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(370), 2,
      anon_sym_PLUS,
      anon_sym_DASH,
    ACTIONS(372), 3,
      anon_sym_STAR,
      anon_sym_SLASH,
      anon_sym_Mod,
    ACTIONS(374), 10,
      anon_sym_AMP,
      aux_sym_binary_expression_token1,
      aux_sym_binary_expression_token2,
      aux_sym_binary_expression_token3,
      anon_sym_LT_GT,
      anon_sym_LT,
      anon_sym_GT,
      anon_sym_LT_EQ,
      anon_sym_GT_EQ,
      sym__equal,
  [7283] = 7,
    ACTIONS(306), 1,
      anon_sym_DOT,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(308), 2,
      anon_sym_PLUS,
      anon_sym_DASH,
    ACTIONS(328), 2,
      anon_sym_LT,
      anon_sym_GT,
    ACTIONS(382), 2,
      anon_sym_RPAREN,
      anon_sym_COMMA,
    ACTIONS(310), 3,
      anon_sym_STAR,
      anon_sym_SLASH,
      anon_sym_Mod,
    ACTIONS(326), 8,
      anon_sym_AMP,
      aux_sym_binary_expression_token1,
      aux_sym_binary_expression_token2,
      aux_sym_binary_expression_token3,
      anon_sym_LT_GT,
      anon_sym_LT_EQ,
      anon_sym_GT_EQ,
      sym__equal,
  [7318] = 7,
    ACTIONS(368), 1,
      anon_sym_DOT,
    ACTIONS(382), 1,
      aux_sym__whitespace_token1,
    ACTIONS(388), 1,
      anon_sym_COMMA,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(370), 2,
      anon_sym_PLUS,
      anon_sym_DASH,
    ACTIONS(372), 3,
      anon_sym_STAR,
      anon_sym_SLASH,
      anon_sym_Mod,
    ACTIONS(374), 10,
      anon_sym_AMP,
      aux_sym_binary_expression_token1,
      aux_sym_binary_expression_token2,
      aux_sym_binary_expression_token3,
      anon_sym_LT_GT,
      anon_sym_LT,
      anon_sym_GT,
      anon_sym_LT_EQ,
      anon_sym_GT_EQ,
      sym__equal,
  [7353] = 7,
    ACTIONS(306), 1,
      anon_sym_DOT,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(308), 2,
      anon_sym_PLUS,
      anon_sym_DASH,
    ACTIONS(328), 2,
      anon_sym_LT,
      anon_sym_GT,
    ACTIONS(386), 2,
      anon_sym_RPAREN,
      anon_sym_COMMA,
    ACTIONS(310), 3,
      anon_sym_STAR,
      anon_sym_SLASH,
      anon_sym_Mod,
    ACTIONS(326), 8,
      anon_sym_AMP,
      aux_sym_binary_expression_token1,
      aux_sym_binary_expression_token2,
      aux_sym_binary_expression_token3,
      anon_sym_LT_GT,
      anon_sym_LT_EQ,
      anon_sym_GT_EQ,
      sym__equal,
  [7388] = 7,
    ACTIONS(368), 1,
      anon_sym_DOT,
    ACTIONS(390), 1,
      aux_sym__whitespace_token1,
    STATE(12), 1,
      aux_sym__whitespace,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(370), 2,
      anon_sym_PLUS,
      anon_sym_DASH,
    ACTIONS(372), 3,
      anon_sym_STAR,
      anon_sym_SLASH,
      anon_sym_Mod,
    ACTIONS(374), 10,
      anon_sym_AMP,
      aux_sym_binary_expression_token1,
      aux_sym_binary_expression_token2,
      aux_sym_binary_expression_token3,
      anon_sym_LT_GT,
      anon_sym_LT,
      anon_sym_GT,
      anon_sym_LT_EQ,
      anon_sym_GT_EQ,
      sym__equal,
  [7423] = 7,
    ACTIONS(368), 1,
      anon_sym_DOT,
    ACTIONS(392), 1,
      aux_sym__whitespace_token1,
    STATE(13), 1,
      aux_sym__whitespace,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(370), 2,
      anon_sym_PLUS,
      anon_sym_DASH,
    ACTIONS(372), 3,
      anon_sym_STAR,
      anon_sym_SLASH,
      anon_sym_Mod,
    ACTIONS(374), 10,
      anon_sym_AMP,
      aux_sym_binary_expression_token1,
      aux_sym_binary_expression_token2,
      aux_sym_binary_expression_token3,
      anon_sym_LT_GT,
      anon_sym_LT,
      anon_sym_GT,
      anon_sym_LT_EQ,
      anon_sym_GT_EQ,
      sym__equal,
  [7458] = 6,
    ACTIONS(368), 1,
      anon_sym_DOT,
    ACTIONS(394), 1,
      aux_sym__whitespace_token1,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(370), 2,
      anon_sym_PLUS,
      anon_sym_DASH,
    ACTIONS(372), 3,
      anon_sym_STAR,
      anon_sym_SLASH,
      anon_sym_Mod,
    ACTIONS(374), 10,
      anon_sym_AMP,
      aux_sym_binary_expression_token1,
      aux_sym_binary_expression_token2,
      aux_sym_binary_expression_token3,
      anon_sym_LT_GT,
      anon_sym_LT,
      anon_sym_GT,
      anon_sym_LT_EQ,
      anon_sym_GT_EQ,
      sym__equal,
  [7490] = 6,
    ACTIONS(368), 1,
      anon_sym_DOT,
    ACTIONS(396), 1,
      aux_sym__whitespace_token1,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(370), 2,
      anon_sym_PLUS,
      anon_sym_DASH,
    ACTIONS(372), 3,
      anon_sym_STAR,
      anon_sym_SLASH,
      anon_sym_Mod,
    ACTIONS(374), 10,
      anon_sym_AMP,
      aux_sym_binary_expression_token1,
      aux_sym_binary_expression_token2,
      aux_sym_binary_expression_token3,
      anon_sym_LT_GT,
      anon_sym_LT,
      anon_sym_GT,
      anon_sym_LT_EQ,
      anon_sym_GT_EQ,
      sym__equal,
  [7522] = 5,
    ACTIONS(277), 1,
      anon_sym_LPAREN,
    ACTIONS(398), 1,
      anon_sym_DOT,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(230), 2,
      anon_sym_LT,
      anon_sym_GT,
    ACTIONS(279), 13,
      anon_sym_PLUS,
      anon_sym_DASH,
      anon_sym_STAR,
      anon_sym_SLASH,
      anon_sym_Mod,
      anon_sym_AMP,
      aux_sym_binary_expression_token1,
      aux_sym_binary_expression_token2,
      aux_sym_binary_expression_token3,
      anon_sym_LT_GT,
      anon_sym_LT_EQ,
      anon_sym_GT_EQ,
      sym__equal,
  [7552] = 7,
    ACTIONS(306), 1,
      anon_sym_DOT,
    ACTIONS(400), 1,
      anon_sym_RPAREN,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(308), 2,
      anon_sym_PLUS,
      anon_sym_DASH,
    ACTIONS(328), 2,
      anon_sym_LT,
      anon_sym_GT,
    ACTIONS(310), 3,
      anon_sym_STAR,
      anon_sym_SLASH,
      anon_sym_Mod,
    ACTIONS(326), 8,
      anon_sym_AMP,
      aux_sym_binary_expression_token1,
      aux_sym_binary_expression_token2,
      aux_sym_binary_expression_token3,
      anon_sym_LT_GT,
      anon_sym_LT_EQ,
      anon_sym_GT_EQ,
      sym__equal,
  [7586] = 6,
    ACTIONS(368), 1,
      anon_sym_DOT,
    ACTIONS(402), 1,
      aux_sym__whitespace_token1,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(370), 2,
      anon_sym_PLUS,
      anon_sym_DASH,
    ACTIONS(372), 3,
      anon_sym_STAR,
      anon_sym_SLASH,
      anon_sym_Mod,
    ACTIONS(374), 10,
      anon_sym_AMP,
      aux_sym_binary_expression_token1,
      aux_sym_binary_expression_token2,
      aux_sym_binary_expression_token3,
      anon_sym_LT_GT,
      anon_sym_LT,
      anon_sym_GT,
      anon_sym_LT_EQ,
      anon_sym_GT_EQ,
      sym__equal,
  [7618] = 5,
    ACTIONS(277), 1,
      anon_sym_LPAREN,
    ACTIONS(404), 1,
      anon_sym_DOT,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(230), 2,
      anon_sym_LT,
      anon_sym_GT,
    ACTIONS(279), 13,
      anon_sym_PLUS,
      anon_sym_DASH,
      anon_sym_STAR,
      anon_sym_SLASH,
      anon_sym_Mod,
      anon_sym_AMP,
      aux_sym_binary_expression_token1,
      aux_sym_binary_expression_token2,
      aux_sym_binary_expression_token3,
      anon_sym_LT_GT,
      anon_sym_LT_EQ,
      anon_sym_GT_EQ,
      sym__equal,
  [7648] = 6,
    ACTIONS(368), 1,
      anon_sym_DOT,
    ACTIONS(406), 1,
      aux_sym__whitespace_token1,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(370), 2,
      anon_sym_PLUS,
      anon_sym_DASH,
    ACTIONS(372), 3,
      anon_sym_STAR,
      anon_sym_SLASH,
      anon_sym_Mod,
    ACTIONS(374), 10,
      anon_sym_AMP,
      aux_sym_binary_expression_token1,
      aux_sym_binary_expression_token2,
      aux_sym_binary_expression_token3,
      anon_sym_LT_GT,
      anon_sym_LT,
      anon_sym_GT,
      anon_sym_LT_EQ,
      anon_sym_GT_EQ,
      sym__equal,
  [7680] = 6,
    ACTIONS(368), 1,
      anon_sym_DOT,
    ACTIONS(408), 1,
      aux_sym__whitespace_token1,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(370), 2,
      anon_sym_PLUS,
      anon_sym_DASH,
    ACTIONS(372), 3,
      anon_sym_STAR,
      anon_sym_SLASH,
      anon_sym_Mod,
    ACTIONS(374), 10,
      anon_sym_AMP,
      aux_sym_binary_expression_token1,
      aux_sym_binary_expression_token2,
      aux_sym_binary_expression_token3,
      anon_sym_LT_GT,
      anon_sym_LT,
      anon_sym_GT,
      anon_sym_LT_EQ,
      anon_sym_GT_EQ,
      sym__equal,
  [7712] = 6,
    ACTIONS(368), 1,
      anon_sym_DOT,
    ACTIONS(410), 1,
      aux_sym__whitespace_token1,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(370), 2,
      anon_sym_PLUS,
      anon_sym_DASH,
    ACTIONS(372), 3,
      anon_sym_STAR,
      anon_sym_SLASH,
      anon_sym_Mod,
    ACTIONS(374), 10,
      anon_sym_AMP,
      aux_sym_binary_expression_token1,
      aux_sym_binary_expression_token2,
      aux_sym_binary_expression_token3,
      anon_sym_LT_GT,
      anon_sym_LT,
      anon_sym_GT,
      anon_sym_LT_EQ,
      anon_sym_GT_EQ,
      sym__equal,
  [7744] = 7,
    ACTIONS(306), 1,
      anon_sym_DOT,
    ACTIONS(412), 1,
      anon_sym_To,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(308), 2,
      anon_sym_PLUS,
      anon_sym_DASH,
    ACTIONS(328), 2,
      anon_sym_LT,
      anon_sym_GT,
    ACTIONS(310), 3,
      anon_sym_STAR,
      anon_sym_SLASH,
      anon_sym_Mod,
    ACTIONS(326), 8,
      anon_sym_AMP,
      aux_sym_binary_expression_token1,
      aux_sym_binary_expression_token2,
      aux_sym_binary_expression_token3,
      anon_sym_LT_GT,
      anon_sym_LT_EQ,
      anon_sym_GT_EQ,
      sym__equal,
  [7778] = 7,
    ACTIONS(306), 1,
      anon_sym_DOT,
    ACTIONS(414), 1,
      anon_sym_RPAREN,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(308), 2,
      anon_sym_PLUS,
      anon_sym_DASH,
    ACTIONS(328), 2,
      anon_sym_LT,
      anon_sym_GT,
    ACTIONS(310), 3,
      anon_sym_STAR,
      anon_sym_SLASH,
      anon_sym_Mod,
    ACTIONS(326), 8,
      anon_sym_AMP,
      aux_sym_binary_expression_token1,
      aux_sym_binary_expression_token2,
      aux_sym_binary_expression_token3,
      anon_sym_LT_GT,
      anon_sym_LT_EQ,
      anon_sym_GT_EQ,
      sym__equal,
  [7812] = 7,
    ACTIONS(306), 1,
      anon_sym_DOT,
    ACTIONS(416), 1,
      anon_sym_RPAREN,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(308), 2,
      anon_sym_PLUS,
      anon_sym_DASH,
    ACTIONS(328), 2,
      anon_sym_LT,
      anon_sym_GT,
    ACTIONS(310), 3,
      anon_sym_STAR,
      anon_sym_SLASH,
      anon_sym_Mod,
    ACTIONS(326), 8,
      anon_sym_AMP,
      aux_sym_binary_expression_token1,
      aux_sym_binary_expression_token2,
      aux_sym_binary_expression_token3,
      anon_sym_LT_GT,
      anon_sym_LT_EQ,
      anon_sym_GT_EQ,
      sym__equal,
  [7846] = 7,
    ACTIONS(306), 1,
      anon_sym_DOT,
    ACTIONS(418), 1,
      anon_sym_Then,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(308), 2,
      anon_sym_PLUS,
      anon_sym_DASH,
    ACTIONS(328), 2,
      anon_sym_LT,
      anon_sym_GT,
    ACTIONS(310), 3,
      anon_sym_STAR,
      anon_sym_SLASH,
      anon_sym_Mod,
    ACTIONS(326), 8,
      anon_sym_AMP,
      aux_sym_binary_expression_token1,
      aux_sym_binary_expression_token2,
      aux_sym_binary_expression_token3,
      anon_sym_LT_GT,
      anon_sym_LT_EQ,
      anon_sym_GT_EQ,
      sym__equal,
  [7880] = 4,
    ACTIONS(398), 1,
      anon_sym_DOT,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(230), 2,
      anon_sym_LT,
      anon_sym_GT,
    ACTIONS(279), 13,
      anon_sym_PLUS,
      anon_sym_DASH,
      anon_sym_STAR,
      anon_sym_SLASH,
      anon_sym_Mod,
      anon_sym_AMP,
      aux_sym_binary_expression_token1,
      aux_sym_binary_expression_token2,
      aux_sym_binary_expression_token3,
      anon_sym_LT_GT,
      anon_sym_LT_EQ,
      anon_sym_GT_EQ,
      sym__equal,
  [7907] = 6,
    ACTIONS(306), 1,
      anon_sym_DOT,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(308), 2,
      anon_sym_PLUS,
      anon_sym_DASH,
    ACTIONS(328), 2,
      anon_sym_LT,
      anon_sym_GT,
    ACTIONS(310), 3,
      anon_sym_STAR,
      anon_sym_SLASH,
      anon_sym_Mod,
    ACTIONS(326), 8,
      anon_sym_AMP,
      aux_sym_binary_expression_token1,
      aux_sym_binary_expression_token2,
      aux_sym_binary_expression_token3,
      anon_sym_LT_GT,
      anon_sym_LT_EQ,
      anon_sym_GT_EQ,
      sym__equal,
  [7938] = 4,
    ACTIONS(420), 1,
      anon_sym_DOT,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(230), 2,
      anon_sym_LT,
      anon_sym_GT,
    ACTIONS(279), 13,
      anon_sym_PLUS,
      anon_sym_DASH,
      anon_sym_STAR,
      anon_sym_SLASH,
      anon_sym_Mod,
      anon_sym_AMP,
      aux_sym_binary_expression_token1,
      aux_sym_binary_expression_token2,
      aux_sym_binary_expression_token3,
      anon_sym_LT_GT,
      anon_sym_LT_EQ,
      anon_sym_GT_EQ,
      sym__equal,
  [7965] = 4,
    ACTIONS(404), 1,
      anon_sym_DOT,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(230), 2,
      anon_sym_LT,
      anon_sym_GT,
    ACTIONS(279), 13,
      anon_sym_PLUS,
      anon_sym_DASH,
      anon_sym_STAR,
      anon_sym_SLASH,
      anon_sym_Mod,
      anon_sym_AMP,
      aux_sym_binary_expression_token1,
      aux_sym_binary_expression_token2,
      aux_sym_binary_expression_token3,
      anon_sym_LT_GT,
      anon_sym_LT_EQ,
      anon_sym_GT_EQ,
      sym__equal,
  [7992] = 8,
    ACTIONS(422), 1,
      anon_sym_RPAREN,
    ACTIONS(426), 1,
      sym_identifier,
    STATE(198), 1,
      sym_new_identifier,
    STATE(220), 1,
      sym_parameter,
    STATE(245), 1,
      sym_modifier,
    STATE(273), 1,
      sym_parameter_list,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(424), 4,
      anon_sym_ByVal,
      anon_sym_ByRef,
      anon_sym_Optional,
      anon_sym_ParamArray,
  [8021] = 8,
    ACTIONS(426), 1,
      sym_identifier,
    ACTIONS(428), 1,
      anon_sym_RPAREN,
    STATE(198), 1,
      sym_new_identifier,
    STATE(220), 1,
      sym_parameter,
    STATE(245), 1,
      sym_modifier,
    STATE(329), 1,
      sym_parameter_list,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(424), 4,
      anon_sym_ByVal,
      anon_sym_ByRef,
      anon_sym_Optional,
      anon_sym_ParamArray,
  [8050] = 8,
    ACTIONS(426), 1,
      sym_identifier,
    ACTIONS(430), 1,
      anon_sym_RPAREN,
    STATE(198), 1,
      sym_new_identifier,
    STATE(220), 1,
      sym_parameter,
    STATE(245), 1,
      sym_modifier,
    STATE(285), 1,
      sym_parameter_list,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(424), 4,
      anon_sym_ByVal,
      anon_sym_ByRef,
      anon_sym_Optional,
      anon_sym_ParamArray,
  [8079] = 8,
    ACTIONS(426), 1,
      sym_identifier,
    ACTIONS(432), 1,
      anon_sym_RPAREN,
    STATE(198), 1,
      sym_new_identifier,
    STATE(220), 1,
      sym_parameter,
    STATE(245), 1,
      sym_modifier,
    STATE(303), 1,
      sym_parameter_list,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(424), 4,
      anon_sym_ByVal,
      anon_sym_ByRef,
      anon_sym_Optional,
      anon_sym_ParamArray,
  [8108] = 8,
    ACTIONS(426), 1,
      sym_identifier,
    ACTIONS(434), 1,
      anon_sym_RPAREN,
    STATE(198), 1,
      sym_new_identifier,
    STATE(220), 1,
      sym_parameter,
    STATE(245), 1,
      sym_modifier,
    STATE(282), 1,
      sym_parameter_list,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(424), 4,
      anon_sym_ByVal,
      anon_sym_ByRef,
      anon_sym_Optional,
      anon_sym_ParamArray,
  [8137] = 6,
    ACTIONS(426), 1,
      sym_identifier,
    STATE(198), 1,
      sym_new_identifier,
    STATE(245), 1,
      sym_modifier,
    STATE(253), 1,
      sym_parameter,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(424), 4,
      anon_sym_ByVal,
      anon_sym_ByRef,
      anon_sym_Optional,
      anon_sym_ParamArray,
  [8160] = 6,
    ACTIONS(436), 1,
      sym_identifier,
    STATE(184), 1,
      sym_variable_declaration_identifier,
    STATE(189), 1,
      sym_new_identifier,
    STATE(190), 1,
      sym_array_identifier,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    STATE(299), 2,
      sym__variable_declaration_assignment,
      sym_variable_list,
  [8181] = 7,
    ACTIONS(438), 1,
      anon_sym_As,
    ACTIONS(440), 1,
      anon_sym_COMMA,
    ACTIONS(442), 1,
      sym__equal,
    ACTIONS(444), 1,
      aux_sym__whitespace_token1,
    STATE(186), 1,
      aux_sym_variable_list_repeat1,
    STATE(243), 1,
      sym_type_definition,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [8204] = 2,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(446), 5,
      anon_sym_LPAREN,
      anon_sym_RPAREN,
      anon_sym_As,
      anon_sym_COMMA,
      anon_sym_Lib,
  [8216] = 6,
    ACTIONS(438), 1,
      anon_sym_As,
    ACTIONS(440), 1,
      anon_sym_COMMA,
    ACTIONS(448), 1,
      aux_sym__whitespace_token1,
    STATE(193), 1,
      aux_sym_variable_list_repeat1,
    STATE(278), 1,
      sym_type_definition,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [8236] = 2,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(450), 5,
      anon_sym_For,
      anon_sym_Function,
      anon_sym_Sub,
      anon_sym_Do,
      anon_sym_While,
  [8248] = 3,
    ACTIONS(446), 1,
      aux_sym__whitespace_token1,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(452), 4,
      anon_sym_LPAREN,
      anon_sym_As,
      anon_sym_COMMA,
      sym__equal,
  [8262] = 4,
    ACTIONS(454), 1,
      anon_sym_LPAREN,
    ACTIONS(458), 1,
      aux_sym__whitespace_token1,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(456), 3,
      anon_sym_As,
      anon_sym_COMMA,
      sym__equal,
  [8278] = 3,
    ACTIONS(458), 1,
      aux_sym__whitespace_token1,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(456), 3,
      anon_sym_As,
      anon_sym_COMMA,
      sym__equal,
  [8291] = 3,
    ACTIONS(462), 1,
      aux_sym__whitespace_token1,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(460), 3,
      anon_sym_As,
      anon_sym_COMMA,
      sym__equal,
  [8304] = 5,
    ACTIONS(438), 1,
      anon_sym_As,
    ACTIONS(464), 1,
      aux_sym__whitespace_token1,
    STATE(10), 1,
      aux_sym__whitespace,
    STATE(259), 1,
      sym_type_definition,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [8321] = 5,
    ACTIONS(466), 1,
      anon_sym_As,
    ACTIONS(468), 1,
      anon_sym_COMMA,
    ACTIONS(471), 1,
      aux_sym__whitespace_token1,
    STATE(193), 1,
      aux_sym_variable_list_repeat1,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [8338] = 3,
    ACTIONS(475), 1,
      aux_sym__whitespace_token1,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(473), 3,
      anon_sym_As,
      anon_sym_COMMA,
      sym__equal,
  [8351] = 5,
    ACTIONS(436), 1,
      sym_identifier,
    STATE(189), 1,
      sym_new_identifier,
    STATE(190), 1,
      sym_array_identifier,
    STATE(214), 1,
      sym_variable_declaration_identifier,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [8368] = 5,
    ACTIONS(438), 1,
      anon_sym_As,
    ACTIONS(477), 1,
      aux_sym__whitespace_token1,
    STATE(14), 1,
      aux_sym__whitespace,
    STATE(254), 1,
      sym_type_definition,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [8385] = 4,
    ACTIONS(481), 1,
      anon_sym_As,
    STATE(250), 1,
      sym_type_definition,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(479), 2,
      anon_sym_RPAREN,
      anon_sym_COMMA,
  [8400] = 4,
    ACTIONS(481), 1,
      anon_sym_As,
    STATE(257), 1,
      sym_type_definition,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(483), 2,
      anon_sym_RPAREN,
      anon_sym_COMMA,
  [8415] = 5,
    ACTIONS(438), 1,
      anon_sym_As,
    ACTIONS(485), 1,
      aux_sym__whitespace_token1,
    STATE(9), 1,
      aux_sym__whitespace,
    STATE(252), 1,
      sym_type_definition,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [8432] = 3,
    ACTIONS(489), 1,
      aux_sym__whitespace_token1,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(487), 2,
      anon_sym_LPAREN_RPAREN,
      sym__equal,
  [8444] = 4,
    ACTIONS(491), 1,
      anon_sym_RPAREN,
    ACTIONS(493), 1,
      anon_sym_COMMA,
    STATE(201), 1,
      aux_sym_parameter_list_repeat1,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [8458] = 4,
    ACTIONS(35), 1,
      anon_sym_Call,
    ACTIONS(496), 1,
      sym_identifier,
    STATE(44), 1,
      sym_function_call,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [8472] = 4,
    ACTIONS(498), 1,
      anon_sym_COMMA,
    ACTIONS(500), 1,
      aux_sym__whitespace_token1,
    STATE(211), 1,
      aux_sym_argument_list_repeat1,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [8486] = 2,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(502), 3,
      anon_sym_RPAREN,
      anon_sym_COMMA,
      anon_sym_LPAREN_RPAREN,
  [8496] = 4,
    ACTIONS(504), 1,
      anon_sym_RPAREN,
    ACTIONS(506), 1,
      anon_sym_COMMA,
    STATE(201), 1,
      aux_sym_parameter_list_repeat1,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [8510] = 4,
    ACTIONS(204), 1,
      anon_sym_Call,
    ACTIONS(508), 1,
      sym_identifier,
    STATE(147), 1,
      sym_function_call,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [8524] = 4,
    ACTIONS(510), 1,
      anon_sym_RPAREN,
    ACTIONS(512), 1,
      anon_sym_COMMA,
    STATE(207), 1,
      aux_sym_argument_list_repeat1,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [8538] = 4,
    ACTIONS(515), 1,
      anon_sym_RPAREN,
    ACTIONS(517), 1,
      anon_sym_COMMA,
    STATE(207), 1,
      aux_sym_argument_list_repeat1,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [8552] = 4,
    ACTIONS(510), 1,
      aux_sym__whitespace_token1,
    ACTIONS(519), 1,
      anon_sym_COMMA,
    STATE(209), 1,
      aux_sym_argument_list_repeat1,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [8566] = 2,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(489), 3,
      anon_sym_RPAREN,
      anon_sym_COMMA,
      anon_sym_LPAREN_RPAREN,
  [8576] = 4,
    ACTIONS(498), 1,
      anon_sym_COMMA,
    ACTIONS(515), 1,
      aux_sym__whitespace_token1,
    STATE(209), 1,
      aux_sym_argument_list_repeat1,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [8590] = 2,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(522), 3,
      anon_sym_RPAREN,
      anon_sym_COMMA,
      anon_sym_LPAREN_RPAREN,
  [8600] = 3,
    ACTIONS(526), 1,
      anon_sym_LPAREN_RPAREN,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(524), 2,
      anon_sym_RPAREN,
      anon_sym_COMMA,
  [8612] = 3,
    ACTIONS(471), 1,
      aux_sym__whitespace_token1,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(466), 2,
      anon_sym_As,
      anon_sym_COMMA,
  [8624] = 3,
    ACTIONS(502), 1,
      aux_sym__whitespace_token1,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(528), 2,
      anon_sym_LPAREN_RPAREN,
      sym__equal,
  [8636] = 4,
    ACTIONS(500), 1,
      anon_sym_RPAREN,
    ACTIONS(517), 1,
      anon_sym_COMMA,
    STATE(208), 1,
      aux_sym_argument_list_repeat1,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [8650] = 4,
    ACTIONS(524), 1,
      aux_sym__whitespace_token1,
    ACTIONS(530), 1,
      anon_sym_LPAREN_RPAREN,
    ACTIONS(532), 1,
      sym__equal,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [8664] = 3,
    ACTIONS(522), 1,
      aux_sym__whitespace_token1,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(534), 2,
      anon_sym_LPAREN_RPAREN,
      sym__equal,
  [8676] = 4,
    ACTIONS(125), 1,
      anon_sym_Call,
    ACTIONS(536), 1,
      sym_identifier,
    STATE(87), 1,
      sym_function_call,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [8690] = 4,
    ACTIONS(506), 1,
      anon_sym_COMMA,
    ACTIONS(538), 1,
      anon_sym_RPAREN,
    STATE(205), 1,
      aux_sym_parameter_list_repeat1,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [8704] = 3,
    ACTIONS(540), 1,
      anon_sym_RPAREN,
    ACTIONS(542), 1,
      sym_number,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [8715] = 3,
    ACTIONS(544), 1,
      anon_sym_Else,
    ACTIONS(546), 1,
      anon_sym_EndIf,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [8726] = 3,
    ACTIONS(548), 1,
      anon_sym_As,
    STATE(279), 1,
      sym_type_definition,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [8737] = 2,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(382), 2,
      anon_sym_RPAREN,
      anon_sym_COMMA,
  [8746] = 3,
    ACTIONS(550), 1,
      sym_identifier,
    STATE(306), 1,
      sym_new_identifier,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [8757] = 3,
    ACTIONS(550), 1,
      sym_identifier,
    STATE(304), 1,
      sym_new_identifier,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [8768] = 3,
    ACTIONS(548), 1,
      anon_sym_As,
    STATE(277), 1,
      sym_type_definition,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [8779] = 3,
    ACTIONS(552), 1,
      aux_sym__whitespace_token1,
    STATE(5), 1,
      aux_sym__whitespace,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [8790] = 3,
    ACTIONS(554), 1,
      anon_sym_Function,
    ACTIONS(556), 1,
      anon_sym_Declare,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [8801] = 3,
    ACTIONS(558), 1,
      aux_sym__whitespace_token1,
    STATE(71), 1,
      aux_sym__whitespace,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [8812] = 3,
    ACTIONS(560), 1,
      aux_sym__whitespace_token1,
    STATE(76), 1,
      aux_sym__whitespace,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [8823] = 3,
    ACTIONS(548), 1,
      anon_sym_As,
    STATE(331), 1,
      sym_type_definition,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [8834] = 3,
    ACTIONS(562), 1,
      aux_sym__whitespace_token1,
    STATE(104), 1,
      aux_sym__whitespace,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [8845] = 3,
    ACTIONS(564), 1,
      aux_sym__whitespace_token1,
    STATE(105), 1,
      aux_sym__whitespace,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [8856] = 3,
    ACTIONS(566), 1,
      aux_sym__whitespace_token1,
    STATE(65), 1,
      aux_sym__whitespace,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [8867] = 3,
    ACTIONS(568), 1,
      aux_sym__whitespace_token1,
    STATE(54), 1,
      aux_sym__whitespace,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [8878] = 3,
    ACTIONS(550), 1,
      sym_identifier,
    STATE(291), 1,
      sym_new_identifier,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [8889] = 3,
    ACTIONS(570), 1,
      aux_sym__whitespace_token1,
    STATE(96), 1,
      aux_sym__whitespace,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [8900] = 3,
    ACTIONS(548), 1,
      anon_sym_As,
    STATE(322), 1,
      sym_type_definition,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [8911] = 3,
    ACTIONS(382), 1,
      aux_sym__whitespace_token1,
    ACTIONS(388), 1,
      anon_sym_COMMA,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [8922] = 3,
    ACTIONS(572), 1,
      aux_sym__whitespace_token1,
    STATE(110), 1,
      aux_sym__whitespace,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [8933] = 3,
    ACTIONS(574), 1,
      aux_sym__whitespace_token1,
    STATE(7), 1,
      aux_sym__whitespace,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [8944] = 3,
    ACTIONS(448), 1,
      aux_sym__whitespace_token1,
    ACTIONS(576), 1,
      sym__equal,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [8955] = 3,
    ACTIONS(578), 1,
      anon_sym_DQUOTE,
    STATE(313), 1,
      sym_string_literal,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [8966] = 3,
    ACTIONS(550), 1,
      sym_identifier,
    STATE(197), 1,
      sym_new_identifier,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [8977] = 3,
    ACTIONS(580), 1,
      aux_sym__whitespace_token1,
    STATE(11), 1,
      aux_sym__whitespace,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [8988] = 2,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(510), 2,
      anon_sym_RPAREN,
      anon_sym_COMMA,
  [8997] = 2,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(582), 2,
      anon_sym_While,
      anon_sym_Until,
  [9006] = 3,
    ACTIONS(550), 1,
      sym_identifier,
    STATE(272), 1,
      sym_new_identifier,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9017] = 2,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(584), 2,
      anon_sym_RPAREN,
      anon_sym_COMMA,
  [9026] = 3,
    ACTIONS(586), 1,
      anon_sym_LPAREN,
    ACTIONS(588), 1,
      anon_sym_Alias,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9037] = 3,
    ACTIONS(590), 1,
      aux_sym__whitespace_token1,
    STATE(15), 1,
      aux_sym__whitespace,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9048] = 2,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(491), 2,
      anon_sym_RPAREN,
      anon_sym_COMMA,
  [9057] = 3,
    ACTIONS(485), 1,
      aux_sym__whitespace_token1,
    STATE(9), 1,
      aux_sym__whitespace,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9068] = 3,
    ACTIONS(510), 1,
      aux_sym__whitespace_token1,
    ACTIONS(592), 1,
      anon_sym_COMMA,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9079] = 3,
    ACTIONS(594), 1,
      aux_sym__whitespace_token1,
    STATE(6), 1,
      aux_sym__whitespace,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9090] = 2,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
    ACTIONS(479), 2,
      anon_sym_RPAREN,
      anon_sym_COMMA,
  [9099] = 3,
    ACTIONS(578), 1,
      anon_sym_DQUOTE,
    STATE(251), 1,
      sym_string_literal,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9110] = 3,
    ACTIONS(477), 1,
      aux_sym__whitespace_token1,
    STATE(14), 1,
      aux_sym__whitespace,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9121] = 3,
    ACTIONS(596), 1,
      aux_sym__whitespace_token1,
    STATE(8), 1,
      aux_sym__whitespace,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9132] = 2,
    ACTIONS(598), 1,
      anon_sym_To,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9140] = 2,
    ACTIONS(600), 1,
      anon_sym_LPAREN,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9148] = 2,
    ACTIONS(602), 1,
      anon_sym_EndSub,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9156] = 2,
    ACTIONS(604), 1,
      aux_sym__whitespace_token1,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9164] = 2,
    ACTIONS(606), 1,
      aux_sym__whitespace_token1,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9172] = 2,
    ACTIONS(608), 1,
      anon_sym_EndFunction,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9180] = 2,
    ACTIONS(610), 1,
      anon_sym_RPAREN,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9188] = 2,
    ACTIONS(612), 1,
      aux_sym__whitespace_token1,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9196] = 2,
    ACTIONS(614), 1,
      anon_sym_EndSub,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9204] = 2,
    ACTIONS(616), 1,
      anon_sym_EndFunction,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9212] = 2,
    ACTIONS(618), 1,
      sym_number,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9220] = 2,
    ACTIONS(620), 1,
      anon_sym_Lib,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9228] = 2,
    ACTIONS(622), 1,
      anon_sym_RPAREN,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9236] = 2,
    ACTIONS(624), 1,
      aux_sym__whitespace_token1,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9244] = 2,
    ACTIONS(626), 1,
      sym_identifier,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9252] = 2,
    ACTIONS(628), 1,
      anon_sym_RPAREN,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9260] = 2,
    ACTIONS(630), 1,
      aux_sym__whitespace_token1,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9268] = 2,
    ACTIONS(632), 1,
      aux_sym__whitespace_token1,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9276] = 2,
    ACTIONS(634), 1,
      aux_sym__whitespace_token1,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9284] = 2,
    ACTIONS(636), 1,
      anon_sym_Wend,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9292] = 2,
    ACTIONS(638), 1,
      aux_sym__whitespace_token1,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9300] = 2,
    ACTIONS(640), 1,
      anon_sym_RPAREN,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9308] = 2,
    ACTIONS(642), 1,
      anon_sym_EndFunction,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9316] = 2,
    ACTIONS(644), 1,
      aux_sym__whitespace_token1,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9324] = 2,
    ACTIONS(646), 1,
      anon_sym_RPAREN,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9332] = 2,
    ACTIONS(648), 1,
      sym_identifier,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9340] = 2,
    ACTIONS(650), 1,
      anon_sym_RPAREN,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9348] = 2,
    ACTIONS(652), 1,
      sym_identifier,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9356] = 2,
    ACTIONS(654), 1,
      anon_sym_EndIf,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9364] = 2,
    ACTIONS(656), 1,
      anon_sym_Function,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9372] = 2,
    ACTIONS(658), 1,
      anon_sym_LPAREN,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9380] = 2,
    ACTIONS(660), 1,
      sym_identifier,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9388] = 2,
    ACTIONS(662), 1,
      anon_sym_Loop,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9396] = 2,
    ACTIONS(664), 1,
      aux_sym__whitespace_token1,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9404] = 2,
    ACTIONS(666), 1,
      aux_sym__whitespace_token1,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9412] = 2,
    ACTIONS(668), 1,
      aux_sym__whitespace_token1,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9420] = 2,
    ACTIONS(670), 1,
      anon_sym_DQUOTE,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9428] = 2,
    ACTIONS(672), 1,
      anon_sym_Next,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9436] = 2,
    ACTIONS(674), 1,
      aux_sym__whitespace_token1,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9444] = 2,
    ACTIONS(676), 1,
      anon_sym_DQUOTE,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9452] = 2,
    ACTIONS(678), 1,
      anon_sym_PtrSafe,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9460] = 2,
    ACTIONS(680), 1,
      anon_sym_EndFunction,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9468] = 2,
    ACTIONS(682), 1,
      anon_sym_RPAREN,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9476] = 2,
    ACTIONS(684), 1,
      anon_sym_LPAREN,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9484] = 2,
    ACTIONS(686), 1,
      sym_identifier,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9492] = 2,
    ACTIONS(688), 1,
      anon_sym_LPAREN,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9500] = 2,
    ACTIONS(690), 1,
      anon_sym_RPAREN,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9508] = 2,
    ACTIONS(692), 1,
      anon_sym_RPAREN,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9516] = 2,
    ACTIONS(694), 1,
      sym__equal,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9524] = 2,
    ACTIONS(696), 1,
      aux_sym__whitespace_token1,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9532] = 2,
    ACTIONS(698), 1,
      sym__equal,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9540] = 2,
    ACTIONS(700), 1,
      anon_sym_DQUOTE,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9548] = 2,
    ACTIONS(702), 1,
      anon_sym_LPAREN,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9556] = 2,
    ACTIONS(704), 1,
      anon_sym_Next,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9564] = 2,
    ACTIONS(706), 1,
      ts_builtin_sym_end,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9572] = 2,
    ACTIONS(708), 1,
      sym_identifier,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9580] = 2,
    ACTIONS(710), 1,
      aux_sym_string_literal_token1,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9588] = 2,
    ACTIONS(712), 1,
      anon_sym_RPAREN,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9596] = 2,
    ACTIONS(714), 1,
      anon_sym_RPAREN,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9604] = 2,
    ACTIONS(716), 1,
      aux_sym__whitespace_token1,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9612] = 2,
    ACTIONS(718), 1,
      aux_sym__whitespace_token1,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9620] = 2,
    ACTIONS(720), 1,
      aux_sym__whitespace_token1,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9628] = 2,
    ACTIONS(722), 1,
      sym_identifier,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9636] = 2,
    ACTIONS(724), 1,
      sym_identifier,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9644] = 2,
    ACTIONS(726), 1,
      aux_sym_string_literal_token1,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9652] = 2,
    ACTIONS(728), 1,
      sym__equal,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9660] = 2,
    ACTIONS(730), 1,
      anon_sym_LPAREN,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9668] = 2,
    ACTIONS(732), 1,
      aux_sym__whitespace_token1,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9676] = 2,
    ACTIONS(734), 1,
      anon_sym_RPAREN,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9684] = 2,
    ACTIONS(736), 1,
      aux_sym_string_literal_token1,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9692] = 2,
    ACTIONS(738), 1,
      aux_sym__whitespace_token1,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9700] = 2,
    ACTIONS(740), 1,
      anon_sym_LPAREN,
    ACTIONS(3), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9708] = 2,
    ACTIONS(742), 1,
      sym_identifier,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
  [9716] = 2,
    ACTIONS(744), 1,
      sym_identifier,
    ACTIONS(7), 2,
      sym_comment,
      sym__horizontal_whitespace,
};

static const uint32_t ts_small_parse_table_map[] = {
  [SMALL_STATE(2)] = 0,
  [SMALL_STATE(3)] = 99,
  [SMALL_STATE(4)] = 198,
  [SMALL_STATE(5)] = 290,
  [SMALL_STATE(6)] = 382,
  [SMALL_STATE(7)] = 474,
  [SMALL_STATE(8)] = 566,
  [SMALL_STATE(9)] = 658,
  [SMALL_STATE(10)] = 750,
  [SMALL_STATE(11)] = 842,
  [SMALL_STATE(12)] = 934,
  [SMALL_STATE(13)] = 1026,
  [SMALL_STATE(14)] = 1118,
  [SMALL_STATE(15)] = 1210,
  [SMALL_STATE(16)] = 1302,
  [SMALL_STATE(17)] = 1377,
  [SMALL_STATE(18)] = 1452,
  [SMALL_STATE(19)] = 1539,
  [SMALL_STATE(20)] = 1626,
  [SMALL_STATE(21)] = 1712,
  [SMALL_STATE(22)] = 1798,
  [SMALL_STATE(23)] = 1884,
  [SMALL_STATE(24)] = 1970,
  [SMALL_STATE(25)] = 2056,
  [SMALL_STATE(26)] = 2142,
  [SMALL_STATE(27)] = 2228,
  [SMALL_STATE(28)] = 2314,
  [SMALL_STATE(29)] = 2400,
  [SMALL_STATE(30)] = 2486,
  [SMALL_STATE(31)] = 2572,
  [SMALL_STATE(32)] = 2658,
  [SMALL_STATE(33)] = 2739,
  [SMALL_STATE(34)] = 2773,
  [SMALL_STATE(35)] = 2807,
  [SMALL_STATE(36)] = 2841,
  [SMALL_STATE(37)] = 2877,
  [SMALL_STATE(38)] = 2917,
  [SMALL_STATE(39)] = 2951,
  [SMALL_STATE(40)] = 2991,
  [SMALL_STATE(41)] = 3029,
  [SMALL_STATE(42)] = 3065,
  [SMALL_STATE(43)] = 3099,
  [SMALL_STATE(44)] = 3133,
  [SMALL_STATE(45)] = 3167,
  [SMALL_STATE(46)] = 3201,
  [SMALL_STATE(47)] = 3235,
  [SMALL_STATE(48)] = 3277,
  [SMALL_STATE(49)] = 3313,
  [SMALL_STATE(50)] = 3347,
  [SMALL_STATE(51)] = 3381,
  [SMALL_STATE(52)] = 3418,
  [SMALL_STATE(53)] = 3474,
  [SMALL_STATE(54)] = 3530,
  [SMALL_STATE(55)] = 3566,
  [SMALL_STATE(56)] = 3622,
  [SMALL_STATE(57)] = 3678,
  [SMALL_STATE(58)] = 3734,
  [SMALL_STATE(59)] = 3770,
  [SMALL_STATE(60)] = 3826,
  [SMALL_STATE(61)] = 3882,
  [SMALL_STATE(62)] = 3913,
  [SMALL_STATE(63)] = 3945,
  [SMALL_STATE(64)] = 3995,
  [SMALL_STATE(65)] = 4027,
  [SMALL_STATE(66)] = 4059,
  [SMALL_STATE(67)] = 4091,
  [SMALL_STATE(68)] = 4141,
  [SMALL_STATE(69)] = 4173,
  [SMALL_STATE(70)] = 4207,
  [SMALL_STATE(71)] = 4237,
  [SMALL_STATE(72)] = 4268,
  [SMALL_STATE(73)] = 4301,
  [SMALL_STATE(74)] = 4334,
  [SMALL_STATE(75)] = 4365,
  [SMALL_STATE(76)] = 4396,
  [SMALL_STATE(77)] = 4427,
  [SMALL_STATE(78)] = 4476,
  [SMALL_STATE(79)] = 4505,
  [SMALL_STATE(80)] = 4540,
  [SMALL_STATE(81)] = 4571,
  [SMALL_STATE(82)] = 4600,
  [SMALL_STATE(83)] = 4629,
  [SMALL_STATE(84)] = 4658,
  [SMALL_STATE(85)] = 4691,
  [SMALL_STATE(86)] = 4728,
  [SMALL_STATE(87)] = 4757,
  [SMALL_STATE(88)] = 4786,
  [SMALL_STATE(89)] = 4815,
  [SMALL_STATE(90)] = 4848,
  [SMALL_STATE(91)] = 4879,
  [SMALL_STATE(92)] = 4928,
  [SMALL_STATE(93)] = 4957,
  [SMALL_STATE(94)] = 4986,
  [SMALL_STATE(95)] = 5035,
  [SMALL_STATE(96)] = 5082,
  [SMALL_STATE(97)] = 5113,
  [SMALL_STATE(98)] = 5144,
  [SMALL_STATE(99)] = 5177,
  [SMALL_STATE(100)] = 5210,
  [SMALL_STATE(101)] = 5241,
  [SMALL_STATE(102)] = 5272,
  [SMALL_STATE(103)] = 5303,
  [SMALL_STATE(104)] = 5336,
  [SMALL_STATE(105)] = 5367,
  [SMALL_STATE(106)] = 5398,
  [SMALL_STATE(107)] = 5429,
  [SMALL_STATE(108)] = 5460,
  [SMALL_STATE(109)] = 5493,
  [SMALL_STATE(110)] = 5526,
  [SMALL_STATE(111)] = 5557,
  [SMALL_STATE(112)] = 5601,
  [SMALL_STATE(113)] = 5645,
  [SMALL_STATE(114)] = 5673,
  [SMALL_STATE(115)] = 5701,
  [SMALL_STATE(116)] = 5745,
  [SMALL_STATE(117)] = 5789,
  [SMALL_STATE(118)] = 5833,
  [SMALL_STATE(119)] = 5877,
  [SMALL_STATE(120)] = 5905,
  [SMALL_STATE(121)] = 5933,
  [SMALL_STATE(122)] = 5977,
  [SMALL_STATE(123)] = 6021,
  [SMALL_STATE(124)] = 6065,
  [SMALL_STATE(125)] = 6109,
  [SMALL_STATE(126)] = 6153,
  [SMALL_STATE(127)] = 6197,
  [SMALL_STATE(128)] = 6233,
  [SMALL_STATE(129)] = 6277,
  [SMALL_STATE(130)] = 6321,
  [SMALL_STATE(131)] = 6365,
  [SMALL_STATE(132)] = 6399,
  [SMALL_STATE(133)] = 6437,
  [SMALL_STATE(134)] = 6465,
  [SMALL_STATE(135)] = 6509,
  [SMALL_STATE(136)] = 6553,
  [SMALL_STATE(137)] = 6597,
  [SMALL_STATE(138)] = 6641,
  [SMALL_STATE(139)] = 6669,
  [SMALL_STATE(140)] = 6699,
  [SMALL_STATE(141)] = 6727,
  [SMALL_STATE(142)] = 6771,
  [SMALL_STATE(143)] = 6815,
  [SMALL_STATE(144)] = 6847,
  [SMALL_STATE(145)] = 6891,
  [SMALL_STATE(146)] = 6935,
  [SMALL_STATE(147)] = 6979,
  [SMALL_STATE(148)] = 7007,
  [SMALL_STATE(149)] = 7051,
  [SMALL_STATE(150)] = 7095,
  [SMALL_STATE(151)] = 7139,
  [SMALL_STATE(152)] = 7167,
  [SMALL_STATE(153)] = 7211,
  [SMALL_STATE(154)] = 7248,
  [SMALL_STATE(155)] = 7283,
  [SMALL_STATE(156)] = 7318,
  [SMALL_STATE(157)] = 7353,
  [SMALL_STATE(158)] = 7388,
  [SMALL_STATE(159)] = 7423,
  [SMALL_STATE(160)] = 7458,
  [SMALL_STATE(161)] = 7490,
  [SMALL_STATE(162)] = 7522,
  [SMALL_STATE(163)] = 7552,
  [SMALL_STATE(164)] = 7586,
  [SMALL_STATE(165)] = 7618,
  [SMALL_STATE(166)] = 7648,
  [SMALL_STATE(167)] = 7680,
  [SMALL_STATE(168)] = 7712,
  [SMALL_STATE(169)] = 7744,
  [SMALL_STATE(170)] = 7778,
  [SMALL_STATE(171)] = 7812,
  [SMALL_STATE(172)] = 7846,
  [SMALL_STATE(173)] = 7880,
  [SMALL_STATE(174)] = 7907,
  [SMALL_STATE(175)] = 7938,
  [SMALL_STATE(176)] = 7965,
  [SMALL_STATE(177)] = 7992,
  [SMALL_STATE(178)] = 8021,
  [SMALL_STATE(179)] = 8050,
  [SMALL_STATE(180)] = 8079,
  [SMALL_STATE(181)] = 8108,
  [SMALL_STATE(182)] = 8137,
  [SMALL_STATE(183)] = 8160,
  [SMALL_STATE(184)] = 8181,
  [SMALL_STATE(185)] = 8204,
  [SMALL_STATE(186)] = 8216,
  [SMALL_STATE(187)] = 8236,
  [SMALL_STATE(188)] = 8248,
  [SMALL_STATE(189)] = 8262,
  [SMALL_STATE(190)] = 8278,
  [SMALL_STATE(191)] = 8291,
  [SMALL_STATE(192)] = 8304,
  [SMALL_STATE(193)] = 8321,
  [SMALL_STATE(194)] = 8338,
  [SMALL_STATE(195)] = 8351,
  [SMALL_STATE(196)] = 8368,
  [SMALL_STATE(197)] = 8385,
  [SMALL_STATE(198)] = 8400,
  [SMALL_STATE(199)] = 8415,
  [SMALL_STATE(200)] = 8432,
  [SMALL_STATE(201)] = 8444,
  [SMALL_STATE(202)] = 8458,
  [SMALL_STATE(203)] = 8472,
  [SMALL_STATE(204)] = 8486,
  [SMALL_STATE(205)] = 8496,
  [SMALL_STATE(206)] = 8510,
  [SMALL_STATE(207)] = 8524,
  [SMALL_STATE(208)] = 8538,
  [SMALL_STATE(209)] = 8552,
  [SMALL_STATE(210)] = 8566,
  [SMALL_STATE(211)] = 8576,
  [SMALL_STATE(212)] = 8590,
  [SMALL_STATE(213)] = 8600,
  [SMALL_STATE(214)] = 8612,
  [SMALL_STATE(215)] = 8624,
  [SMALL_STATE(216)] = 8636,
  [SMALL_STATE(217)] = 8650,
  [SMALL_STATE(218)] = 8664,
  [SMALL_STATE(219)] = 8676,
  [SMALL_STATE(220)] = 8690,
  [SMALL_STATE(221)] = 8704,
  [SMALL_STATE(222)] = 8715,
  [SMALL_STATE(223)] = 8726,
  [SMALL_STATE(224)] = 8737,
  [SMALL_STATE(225)] = 8746,
  [SMALL_STATE(226)] = 8757,
  [SMALL_STATE(227)] = 8768,
  [SMALL_STATE(228)] = 8779,
  [SMALL_STATE(229)] = 8790,
  [SMALL_STATE(230)] = 8801,
  [SMALL_STATE(231)] = 8812,
  [SMALL_STATE(232)] = 8823,
  [SMALL_STATE(233)] = 8834,
  [SMALL_STATE(234)] = 8845,
  [SMALL_STATE(235)] = 8856,
  [SMALL_STATE(236)] = 8867,
  [SMALL_STATE(237)] = 8878,
  [SMALL_STATE(238)] = 8889,
  [SMALL_STATE(239)] = 8900,
  [SMALL_STATE(240)] = 8911,
  [SMALL_STATE(241)] = 8922,
  [SMALL_STATE(242)] = 8933,
  [SMALL_STATE(243)] = 8944,
  [SMALL_STATE(244)] = 8955,
  [SMALL_STATE(245)] = 8966,
  [SMALL_STATE(246)] = 8977,
  [SMALL_STATE(247)] = 8988,
  [SMALL_STATE(248)] = 8997,
  [SMALL_STATE(249)] = 9006,
  [SMALL_STATE(250)] = 9017,
  [SMALL_STATE(251)] = 9026,
  [SMALL_STATE(252)] = 9037,
  [SMALL_STATE(253)] = 9048,
  [SMALL_STATE(254)] = 9057,
  [SMALL_STATE(255)] = 9068,
  [SMALL_STATE(256)] = 9079,
  [SMALL_STATE(257)] = 9090,
  [SMALL_STATE(258)] = 9099,
  [SMALL_STATE(259)] = 9110,
  [SMALL_STATE(260)] = 9121,
  [SMALL_STATE(261)] = 9132,
  [SMALL_STATE(262)] = 9140,
  [SMALL_STATE(263)] = 9148,
  [SMALL_STATE(264)] = 9156,
  [SMALL_STATE(265)] = 9164,
  [SMALL_STATE(266)] = 9172,
  [SMALL_STATE(267)] = 9180,
  [SMALL_STATE(268)] = 9188,
  [SMALL_STATE(269)] = 9196,
  [SMALL_STATE(270)] = 9204,
  [SMALL_STATE(271)] = 9212,
  [SMALL_STATE(272)] = 9220,
  [SMALL_STATE(273)] = 9228,
  [SMALL_STATE(274)] = 9236,
  [SMALL_STATE(275)] = 9244,
  [SMALL_STATE(276)] = 9252,
  [SMALL_STATE(277)] = 9260,
  [SMALL_STATE(278)] = 9268,
  [SMALL_STATE(279)] = 9276,
  [SMALL_STATE(280)] = 9284,
  [SMALL_STATE(281)] = 9292,
  [SMALL_STATE(282)] = 9300,
  [SMALL_STATE(283)] = 9308,
  [SMALL_STATE(284)] = 9316,
  [SMALL_STATE(285)] = 9324,
  [SMALL_STATE(286)] = 9332,
  [SMALL_STATE(287)] = 9340,
  [SMALL_STATE(288)] = 9348,
  [SMALL_STATE(289)] = 9356,
  [SMALL_STATE(290)] = 9364,
  [SMALL_STATE(291)] = 9372,
  [SMALL_STATE(292)] = 9380,
  [SMALL_STATE(293)] = 9388,
  [SMALL_STATE(294)] = 9396,
  [SMALL_STATE(295)] = 9404,
  [SMALL_STATE(296)] = 9412,
  [SMALL_STATE(297)] = 9420,
  [SMALL_STATE(298)] = 9428,
  [SMALL_STATE(299)] = 9436,
  [SMALL_STATE(300)] = 9444,
  [SMALL_STATE(301)] = 9452,
  [SMALL_STATE(302)] = 9460,
  [SMALL_STATE(303)] = 9468,
  [SMALL_STATE(304)] = 9476,
  [SMALL_STATE(305)] = 9484,
  [SMALL_STATE(306)] = 9492,
  [SMALL_STATE(307)] = 9500,
  [SMALL_STATE(308)] = 9508,
  [SMALL_STATE(309)] = 9516,
  [SMALL_STATE(310)] = 9524,
  [SMALL_STATE(311)] = 9532,
  [SMALL_STATE(312)] = 9540,
  [SMALL_STATE(313)] = 9548,
  [SMALL_STATE(314)] = 9556,
  [SMALL_STATE(315)] = 9564,
  [SMALL_STATE(316)] = 9572,
  [SMALL_STATE(317)] = 9580,
  [SMALL_STATE(318)] = 9588,
  [SMALL_STATE(319)] = 9596,
  [SMALL_STATE(320)] = 9604,
  [SMALL_STATE(321)] = 9612,
  [SMALL_STATE(322)] = 9620,
  [SMALL_STATE(323)] = 9628,
  [SMALL_STATE(324)] = 9636,
  [SMALL_STATE(325)] = 9644,
  [SMALL_STATE(326)] = 9652,
  [SMALL_STATE(327)] = 9660,
  [SMALL_STATE(328)] = 9668,
  [SMALL_STATE(329)] = 9676,
  [SMALL_STATE(330)] = 9684,
  [SMALL_STATE(331)] = 9692,
  [SMALL_STATE(332)] = 9700,
  [SMALL_STATE(333)] = 9708,
  [SMALL_STATE(334)] = 9716,
};

static const TSParseActionEntry ts_parse_actions[] = {
  [0] = {.entry = {.count = 0, .reusable = false}},
  [1] = {.entry = {.count = 1, .reusable = false}}, RECOVER(),
  [3] = {.entry = {.count = 1, .reusable = true}}, SHIFT_EXTRA(),
  [5] = {.entry = {.count = 1, .reusable = true}}, REDUCE(sym_source_file, 0, 0, 0),
  [7] = {.entry = {.count = 1, .reusable = false}}, SHIFT_EXTRA(),
  [9] = {.entry = {.count = 1, .reusable = false}}, SHIFT(187),
  [11] = {.entry = {.count = 1, .reusable = false}}, SHIFT(275),
  [13] = {.entry = {.count = 1, .reusable = false}}, SHIFT(225),
  [15] = {.entry = {.count = 1, .reusable = false}}, SHIFT(226),
  [17] = {.entry = {.count = 1, .reusable = false}}, SHIFT(228),
  [19] = {.entry = {.count = 1, .reusable = false}}, SHIFT(137),
  [21] = {.entry = {.count = 1, .reusable = false}}, SHIFT(141),
  [23] = {.entry = {.count = 1, .reusable = false}}, SHIFT(150),
  [25] = {.entry = {.count = 1, .reusable = false}}, SHIFT(229),
  [27] = {.entry = {.count = 1, .reusable = false}}, SHIFT(183),
  [29] = {.entry = {.count = 1, .reusable = false}}, SHIFT(95),
  [31] = {.entry = {.count = 1, .reusable = false}}, SHIFT(94),
  [33] = {.entry = {.count = 1, .reusable = false}}, SHIFT(116),
  [35] = {.entry = {.count = 1, .reusable = false}}, SHIFT(324),
  [37] = {.entry = {.count = 1, .reusable = false}}, SHIFT(46),
  [39] = {.entry = {.count = 1, .reusable = false}}, SHIFT(317),
  [41] = {.entry = {.count = 1, .reusable = false}}, SHIFT(34),
  [43] = {.entry = {.count = 1, .reusable = false}}, SHIFT(51),
  [45] = {.entry = {.count = 1, .reusable = true}}, REDUCE(aux_sym_source_file_repeat1, 2, 0, 0),
  [47] = {.entry = {.count = 2, .reusable = false}}, REDUCE(aux_sym_source_file_repeat1, 2, 0, 0), SHIFT_REPEAT(187),
  [50] = {.entry = {.count = 2, .reusable = false}}, REDUCE(aux_sym_source_file_repeat1, 2, 0, 0), SHIFT_REPEAT(275),
  [53] = {.entry = {.count = 2, .reusable = false}}, REDUCE(aux_sym_source_file_repeat1, 2, 0, 0), SHIFT_REPEAT(225),
  [56] = {.entry = {.count = 2, .reusable = false}}, REDUCE(aux_sym_source_file_repeat1, 2, 0, 0), SHIFT_REPEAT(226),
  [59] = {.entry = {.count = 2, .reusable = false}}, REDUCE(aux_sym_source_file_repeat1, 2, 0, 0), SHIFT_REPEAT(228),
  [62] = {.entry = {.count = 2, .reusable = false}}, REDUCE(aux_sym_source_file_repeat1, 2, 0, 0), SHIFT_REPEAT(137),
  [65] = {.entry = {.count = 2, .reusable = false}}, REDUCE(aux_sym_source_file_repeat1, 2, 0, 0), SHIFT_REPEAT(141),
  [68] = {.entry = {.count = 2, .reusable = false}}, REDUCE(aux_sym_source_file_repeat1, 2, 0, 0), SHIFT_REPEAT(150),
  [71] = {.entry = {.count = 2, .reusable = false}}, REDUCE(aux_sym_source_file_repeat1, 2, 0, 0), SHIFT_REPEAT(229),
  [74] = {.entry = {.count = 2, .reusable = false}}, REDUCE(aux_sym_source_file_repeat1, 2, 0, 0), SHIFT_REPEAT(183),
  [77] = {.entry = {.count = 2, .reusable = false}}, REDUCE(aux_sym_source_file_repeat1, 2, 0, 0), SHIFT_REPEAT(95),
  [80] = {.entry = {.count = 2, .reusable = false}}, REDUCE(aux_sym_source_file_repeat1, 2, 0, 0), SHIFT_REPEAT(94),
  [83] = {.entry = {.count = 2, .reusable = false}}, REDUCE(aux_sym_source_file_repeat1, 2, 0, 0), SHIFT_REPEAT(116),
  [86] = {.entry = {.count = 2, .reusable = false}}, REDUCE(aux_sym_source_file_repeat1, 2, 0, 0), SHIFT_REPEAT(324),
  [89] = {.entry = {.count = 2, .reusable = false}}, REDUCE(aux_sym_source_file_repeat1, 2, 0, 0), SHIFT_REPEAT(46),
  [92] = {.entry = {.count = 2, .reusable = false}}, REDUCE(aux_sym_source_file_repeat1, 2, 0, 0), SHIFT_REPEAT(317),
  [95] = {.entry = {.count = 2, .reusable = false}}, REDUCE(aux_sym_source_file_repeat1, 2, 0, 0), SHIFT_REPEAT(34),
  [98] = {.entry = {.count = 2, .reusable = false}}, REDUCE(aux_sym_source_file_repeat1, 2, 0, 0), SHIFT_REPEAT(51),
  [101] = {.entry = {.count = 1, .reusable = true}}, REDUCE(sym_source_file, 1, 0, 0),
  [103] = {.entry = {.count = 1, .reusable = false}}, SHIFT(89),
  [105] = {.entry = {.count = 1, .reusable = false}}, SHIFT(84),
  [107] = {.entry = {.count = 1, .reusable = false}}, SHIFT(109),
  [109] = {.entry = {.count = 1, .reusable = false}}, SHIFT(69),
  [111] = {.entry = {.count = 1, .reusable = false}}, SHIFT(73),
  [113] = {.entry = {.count = 1, .reusable = false}}, SHIFT(98),
  [115] = {.entry = {.count = 1, .reusable = false}}, SHIFT(72),
  [117] = {.entry = {.count = 1, .reusable = false}}, SHIFT(117),
  [119] = {.entry = {.count = 1, .reusable = false}}, SHIFT(218),
  [121] = {.entry = {.count = 1, .reusable = false}}, SHIFT(91),
  [123] = {.entry = {.count = 1, .reusable = false}}, SHIFT(118),
  [125] = {.entry = {.count = 1, .reusable = false}}, SHIFT(334),
  [127] = {.entry = {.count = 1, .reusable = false}}, SHIFT(92),
  [129] = {.entry = {.count = 1, .reusable = false}}, SHIFT(330),
  [131] = {.entry = {.count = 1, .reusable = false}}, SHIFT(88),
  [133] = {.entry = {.count = 1, .reusable = false}}, SHIFT(162),
  [135] = {.entry = {.count = 1, .reusable = false}}, SHIFT(212),
  [137] = {.entry = {.count = 1, .reusable = false}}, SHIFT(165),
  [139] = {.entry = {.count = 1, .reusable = false}}, REDUCE(sym__inline_statement_block, 1, 0, 0),
  [141] = {.entry = {.count = 2, .reusable = false}}, REDUCE(aux_sym__inline_statement_block_repeat1, 2, 0, 0), SHIFT_REPEAT(187),
  [144] = {.entry = {.count = 2, .reusable = false}}, REDUCE(aux_sym__inline_statement_block_repeat1, 2, 0, 0), SHIFT_REPEAT(275),
  [147] = {.entry = {.count = 2, .reusable = false}}, REDUCE(aux_sym__inline_statement_block_repeat1, 2, 0, 0), SHIFT_REPEAT(228),
  [150] = {.entry = {.count = 2, .reusable = false}}, REDUCE(aux_sym__inline_statement_block_repeat1, 2, 0, 0), SHIFT_REPEAT(137),
  [153] = {.entry = {.count = 2, .reusable = false}}, REDUCE(aux_sym__inline_statement_block_repeat1, 2, 0, 0), SHIFT_REPEAT(141),
  [156] = {.entry = {.count = 1, .reusable = false}}, REDUCE(aux_sym__inline_statement_block_repeat1, 2, 0, 0),
  [158] = {.entry = {.count = 2, .reusable = false}}, REDUCE(aux_sym__inline_statement_block_repeat1, 2, 0, 0), SHIFT_REPEAT(150),
  [161] = {.entry = {.count = 2, .reusable = false}}, REDUCE(aux_sym__inline_statement_block_repeat1, 2, 0, 0), SHIFT_REPEAT(183),
  [164] = {.entry = {.count = 2, .reusable = false}}, REDUCE(aux_sym__inline_statement_block_repeat1, 2, 0, 0), SHIFT_REPEAT(95),
  [167] = {.entry = {.count = 2, .reusable = false}}, REDUCE(aux_sym__inline_statement_block_repeat1, 2, 0, 0), SHIFT_REPEAT(94),
  [170] = {.entry = {.count = 2, .reusable = false}}, REDUCE(aux_sym__inline_statement_block_repeat1, 2, 0, 0), SHIFT_REPEAT(116),
  [173] = {.entry = {.count = 2, .reusable = false}}, REDUCE(aux_sym__inline_statement_block_repeat1, 2, 0, 0), SHIFT_REPEAT(324),
  [176] = {.entry = {.count = 2, .reusable = false}}, REDUCE(aux_sym__inline_statement_block_repeat1, 2, 0, 0), SHIFT_REPEAT(46),
  [179] = {.entry = {.count = 2, .reusable = false}}, REDUCE(aux_sym__inline_statement_block_repeat1, 2, 0, 0), SHIFT_REPEAT(317),
  [182] = {.entry = {.count = 2, .reusable = false}}, REDUCE(aux_sym__inline_statement_block_repeat1, 2, 0, 0), SHIFT_REPEAT(34),
  [185] = {.entry = {.count = 2, .reusable = false}}, REDUCE(aux_sym__inline_statement_block_repeat1, 2, 0, 0), SHIFT_REPEAT(51),
  [188] = {.entry = {.count = 1, .reusable = false}}, SHIFT(115),
  [190] = {.entry = {.count = 1, .reusable = false}}, SHIFT(77),
  [192] = {.entry = {.count = 1, .reusable = false}}, SHIFT(202),
  [194] = {.entry = {.count = 1, .reusable = false}}, SHIFT(152),
  [196] = {.entry = {.count = 1, .reusable = false}}, SHIFT(111),
  [198] = {.entry = {.count = 1, .reusable = false}}, SHIFT(149),
  [200] = {.entry = {.count = 1, .reusable = false}}, SHIFT(148),
  [202] = {.entry = {.count = 1, .reusable = false}}, SHIFT(122),
  [204] = {.entry = {.count = 1, .reusable = false}}, SHIFT(333),
  [206] = {.entry = {.count = 1, .reusable = false}}, SHIFT(140),
  [208] = {.entry = {.count = 1, .reusable = false}}, SHIFT(325),
  [210] = {.entry = {.count = 1, .reusable = false}}, SHIFT(138),
  [212] = {.entry = {.count = 1, .reusable = false}}, SHIFT(103),
  [214] = {.entry = {.count = 1, .reusable = false}}, REDUCE(sym_invocation_statement, 1, 0, 0),
  [216] = {.entry = {.count = 1, .reusable = false}}, REDUCE(sym_function_call, 4, 0, 0),
  [218] = {.entry = {.count = 1, .reusable = false}}, REDUCE(sym_boolean, 1, 0, 0),
  [220] = {.entry = {.count = 1, .reusable = false}}, REDUCE(sym_string_literal, 3, 0, 0),
  [222] = {.entry = {.count = 1, .reusable = false}}, REDUCE(sym_binary_expression, 3, 0, 0),
  [224] = {.entry = {.count = 1, .reusable = false}}, REDUCE(sym_function_call, 3, 0, 0),
  [226] = {.entry = {.count = 1, .reusable = false}}, REDUCE(sym_unary_expression, 2, 0, 0),
  [228] = {.entry = {.count = 1, .reusable = false}}, SHIFT(55),
  [230] = {.entry = {.count = 1, .reusable = false}}, REDUCE(sym__expression, 1, 0, 0),
  [232] = {.entry = {.count = 1, .reusable = false}}, REDUCE(sym_type_member_expression, 3, 0, 0),
  [234] = {.entry = {.count = 1, .reusable = false}}, REDUCE(sym_new_expression, 2, 0, 0),
  [236] = {.entry = {.count = 1, .reusable = false}}, REDUCE(sym_member_expression, 3, 0, 0),
  [238] = {.entry = {.count = 1, .reusable = false}}, REDUCE(sym_literal, 1, 0, 0),
  [240] = {.entry = {.count = 1, .reusable = false}}, REDUCE(sym__expression, 3, 0, 0),
  [242] = {.entry = {.count = 1, .reusable = false}}, REDUCE(sym_function_call, 5, 0, 0),
  [244] = {.entry = {.count = 1, .reusable = false}}, SHIFT(53),
  [246] = {.entry = {.count = 1, .reusable = false}}, SHIFT(146),
  [248] = {.entry = {.count = 1, .reusable = false}}, SHIFT(119),
  [250] = {.entry = {.count = 1, .reusable = false}}, SHIFT(108),
  [252] = {.entry = {.count = 1, .reusable = false}}, SHIFT(38),
  [254] = {.entry = {.count = 1, .reusable = false}}, REDUCE(aux_sym_source_file_repeat1, 2, 0, 0),
  [256] = {.entry = {.count = 1, .reusable = false}}, SHIFT(58),
  [258] = {.entry = {.count = 1, .reusable = false}}, SHIFT(33),
  [260] = {.entry = {.count = 1, .reusable = false}}, SHIFT(81),
  [262] = {.entry = {.count = 1, .reusable = true}}, REDUCE(aux_sym__whitespace, 2, 0, 0),
  [264] = {.entry = {.count = 1, .reusable = false}}, REDUCE(aux_sym__whitespace, 2, 0, 0),
  [266] = {.entry = {.count = 2, .reusable = false}}, REDUCE(aux_sym__whitespace, 2, 0, 0), SHIFT_REPEAT(58),
  [269] = {.entry = {.count = 1, .reusable = false}}, SHIFT(78),
  [271] = {.entry = {.count = 1, .reusable = false}}, SHIFT(151),
  [273] = {.entry = {.count = 1, .reusable = true}}, REDUCE(sym_string_literal, 3, 0, 0),
  [275] = {.entry = {.count = 1, .reusable = true}}, REDUCE(sym_member_expression, 3, 0, 0),
  [277] = {.entry = {.count = 1, .reusable = true}}, SHIFT(59),
  [279] = {.entry = {.count = 1, .reusable = true}}, REDUCE(sym__expression, 1, 0, 0),
  [281] = {.entry = {.count = 1, .reusable = false}}, SHIFT(66),
  [283] = {.entry = {.count = 2, .reusable = false}}, REDUCE(aux_sym__whitespace, 2, 0, 0), SHIFT_REPEAT(66),
  [286] = {.entry = {.count = 1, .reusable = true}}, REDUCE(sym_new_expression, 2, 0, 0),
  [288] = {.entry = {.count = 1, .reusable = true}}, REDUCE(sym_type_member_expression, 3, 0, 0),
  [290] = {.entry = {.count = 1, .reusable = false}}, SHIFT(75),
  [292] = {.entry = {.count = 2, .reusable = false}}, REDUCE(aux_sym__whitespace, 2, 0, 0), SHIFT_REPEAT(74),
  [295] = {.entry = {.count = 2, .reusable = false}}, REDUCE(aux_sym__whitespace, 2, 0, 0), SHIFT_REPEAT(75),
  [298] = {.entry = {.count = 1, .reusable = false}}, SHIFT(97),
  [300] = {.entry = {.count = 1, .reusable = false}}, SHIFT(107),
  [302] = {.entry = {.count = 1, .reusable = true}}, REDUCE(sym_function_call, 3, 0, 0),
  [304] = {.entry = {.count = 1, .reusable = true}}, REDUCE(sym_binary_expression, 3, 0, 0),
  [306] = {.entry = {.count = 1, .reusable = true}}, SHIFT(219),
  [308] = {.entry = {.count = 1, .reusable = true}}, SHIFT(129),
  [310] = {.entry = {.count = 1, .reusable = true}}, SHIFT(112),
  [312] = {.entry = {.count = 2, .reusable = false}}, REDUCE(aux_sym__whitespace, 2, 0, 0), SHIFT_REPEAT(80),
  [315] = {.entry = {.count = 1, .reusable = true}}, REDUCE(sym_function_call, 4, 0, 0),
  [317] = {.entry = {.count = 1, .reusable = true}}, REDUCE(sym__expression, 3, 0, 0),
  [319] = {.entry = {.count = 1, .reusable = true}}, REDUCE(sym_function_call, 5, 0, 0),
  [321] = {.entry = {.count = 2, .reusable = false}}, REDUCE(aux_sym__whitespace, 2, 0, 0), SHIFT_REPEAT(102),
  [324] = {.entry = {.count = 1, .reusable = true}}, REDUCE(sym_unary_expression, 2, 0, 0),
  [326] = {.entry = {.count = 1, .reusable = true}}, SHIFT(134),
  [328] = {.entry = {.count = 1, .reusable = false}}, SHIFT(134),
  [330] = {.entry = {.count = 1, .reusable = true}}, REDUCE(sym_boolean, 1, 0, 0),
  [332] = {.entry = {.count = 2, .reusable = false}}, REDUCE(aux_sym__whitespace, 2, 0, 0), SHIFT_REPEAT(97),
  [335] = {.entry = {.count = 1, .reusable = false}}, SHIFT(60),
  [337] = {.entry = {.count = 1, .reusable = false}}, SHIFT(68),
  [339] = {.entry = {.count = 1, .reusable = true}}, REDUCE(sym_literal, 1, 0, 0),
  [341] = {.entry = {.count = 1, .reusable = false}}, SHIFT(43),
  [343] = {.entry = {.count = 1, .reusable = false}}, SHIFT(142),
  [345] = {.entry = {.count = 1, .reusable = false}}, SHIFT(90),
  [347] = {.entry = {.count = 1, .reusable = false}}, SHIFT(74),
  [349] = {.entry = {.count = 2, .reusable = false}}, REDUCE(aux_sym__whitespace, 2, 0, 0), SHIFT_REPEAT(106),
  [352] = {.entry = {.count = 1, .reusable = false}}, SHIFT(128),
  [354] = {.entry = {.count = 1, .reusable = false}}, SHIFT(80),
  [356] = {.entry = {.count = 1, .reusable = false}}, SHIFT(106),
  [358] = {.entry = {.count = 1, .reusable = false}}, SHIFT(59),
  [360] = {.entry = {.count = 1, .reusable = true}}, SHIFT(130),
  [362] = {.entry = {.count = 1, .reusable = false}}, SHIFT(102),
  [364] = {.entry = {.count = 1, .reusable = false}}, SHIFT(41),
  [366] = {.entry = {.count = 1, .reusable = false}}, SHIFT(64),
  [368] = {.entry = {.count = 1, .reusable = false}}, SHIFT(206),
  [370] = {.entry = {.count = 1, .reusable = false}}, SHIFT(126),
  [372] = {.entry = {.count = 1, .reusable = false}}, SHIFT(125),
  [374] = {.entry = {.count = 1, .reusable = false}}, SHIFT(121),
  [376] = {.entry = {.count = 1, .reusable = false}}, SHIFT(124),
  [378] = {.entry = {.count = 1, .reusable = true}}, SHIFT(4),
  [380] = {.entry = {.count = 1, .reusable = true}}, SHIFT(326),
  [382] = {.entry = {.count = 1, .reusable = true}}, REDUCE(sym_argument, 1, 0, 0),
  [384] = {.entry = {.count = 1, .reusable = false}}, REDUCE(sym_keyword_argument, 3, 0, 0),
  [386] = {.entry = {.count = 1, .reusable = true}}, REDUCE(sym_keyword_argument, 3, 0, 0),
  [388] = {.entry = {.count = 1, .reusable = false}}, REDUCE(sym_argument, 1, 0, 0),
  [390] = {.entry = {.count = 1, .reusable = true}}, SHIFT(12),
  [392] = {.entry = {.count = 1, .reusable = true}}, SHIFT(13),
  [394] = {.entry = {.count = 1, .reusable = true}}, REDUCE(sym_variable_assignment, 3, 0, 0),
  [396] = {.entry = {.count = 1, .reusable = true}}, REDUCE(sym__variable_declaration_assignment, 3, 0, 0),
  [398] = {.entry = {.count = 1, .reusable = true}}, SHIFT(305),
  [400] = {.entry = {.count = 1, .reusable = true}}, SHIFT(82),
  [402] = {.entry = {.count = 1, .reusable = true}}, REDUCE(sym_do_statement, 6, 0, 0),
  [404] = {.entry = {.count = 1, .reusable = true}}, SHIFT(316),
  [406] = {.entry = {.count = 1, .reusable = true}}, REDUCE(sym_redim, 2, 0, 0),
  [408] = {.entry = {.count = 1, .reusable = true}}, REDUCE(sym_redim, 3, 0, 0),
  [410] = {.entry = {.count = 1, .reusable = true}}, REDUCE(sym__variable_declaration_assignment, 4, 0, 0),
  [412] = {.entry = {.count = 1, .reusable = true}}, SHIFT(135),
  [414] = {.entry = {.count = 1, .reusable = true}}, SHIFT(114),
  [416] = {.entry = {.count = 1, .reusable = true}}, SHIFT(49),
  [418] = {.entry = {.count = 1, .reusable = true}}, SHIFT(242),
  [420] = {.entry = {.count = 1, .reusable = true}}, SHIFT(288),
  [422] = {.entry = {.count = 1, .reusable = false}}, SHIFT(196),
  [424] = {.entry = {.count = 1, .reusable = false}}, SHIFT(286),
  [426] = {.entry = {.count = 1, .reusable = false}}, SHIFT(185),
  [428] = {.entry = {.count = 1, .reusable = false}}, SHIFT(227),
  [430] = {.entry = {.count = 1, .reusable = false}}, SHIFT(192),
  [432] = {.entry = {.count = 1, .reusable = false}}, SHIFT(239),
  [434] = {.entry = {.count = 1, .reusable = false}}, SHIFT(246),
  [436] = {.entry = {.count = 1, .reusable = true}}, SHIFT(188),
  [438] = {.entry = {.count = 1, .reusable = false}}, SHIFT(16),
  [440] = {.entry = {.count = 1, .reusable = false}}, SHIFT(195),
  [442] = {.entry = {.count = 1, .reusable = false}}, SHIFT(136),
  [444] = {.entry = {.count = 1, .reusable = true}}, REDUCE(sym_variable_list, 1, 0, 0),
  [446] = {.entry = {.count = 1, .reusable = true}}, REDUCE(sym_new_identifier, 1, 0, 0),
  [448] = {.entry = {.count = 1, .reusable = true}}, REDUCE(sym_variable_list, 2, 0, 0),
  [450] = {.entry = {.count = 1, .reusable = true}}, SHIFT(310),
  [452] = {.entry = {.count = 1, .reusable = false}}, REDUCE(sym_new_identifier, 1, 0, 0),
  [454] = {.entry = {.count = 1, .reusable = false}}, SHIFT(221),
  [456] = {.entry = {.count = 1, .reusable = false}}, REDUCE(sym_variable_declaration_identifier, 1, 0, 0),
  [458] = {.entry = {.count = 1, .reusable = true}}, REDUCE(sym_variable_declaration_identifier, 1, 0, 0),
  [460] = {.entry = {.count = 1, .reusable = false}}, REDUCE(sym_array_identifier, 6, 0, 0),
  [462] = {.entry = {.count = 1, .reusable = true}}, REDUCE(sym_array_identifier, 6, 0, 0),
  [464] = {.entry = {.count = 1, .reusable = true}}, SHIFT(10),
  [466] = {.entry = {.count = 1, .reusable = false}}, REDUCE(aux_sym_variable_list_repeat1, 2, 0, 0),
  [468] = {.entry = {.count = 2, .reusable = false}}, REDUCE(aux_sym_variable_list_repeat1, 2, 0, 0), SHIFT_REPEAT(195),
  [471] = {.entry = {.count = 1, .reusable = true}}, REDUCE(aux_sym_variable_list_repeat1, 2, 0, 0),
  [473] = {.entry = {.count = 1, .reusable = false}}, REDUCE(sym_array_identifier, 3, 0, 0),
  [475] = {.entry = {.count = 1, .reusable = true}}, REDUCE(sym_array_identifier, 3, 0, 0),
  [477] = {.entry = {.count = 1, .reusable = true}}, SHIFT(14),
  [479] = {.entry = {.count = 1, .reusable = true}}, REDUCE(sym_parameter, 2, 0, 0),
  [481] = {.entry = {.count = 1, .reusable = true}}, SHIFT(17),
  [483] = {.entry = {.count = 1, .reusable = true}}, REDUCE(sym_parameter, 1, 0, 0),
  [485] = {.entry = {.count = 1, .reusable = true}}, SHIFT(9),
  [487] = {.entry = {.count = 1, .reusable = false}}, REDUCE(sym_array_type, 2, 0, 0),
  [489] = {.entry = {.count = 1, .reusable = true}}, REDUCE(sym_array_type, 2, 0, 0),
  [491] = {.entry = {.count = 1, .reusable = true}}, REDUCE(aux_sym_parameter_list_repeat1, 2, 0, 0),
  [493] = {.entry = {.count = 2, .reusable = true}}, REDUCE(aux_sym_parameter_list_repeat1, 2, 0, 0), SHIFT_REPEAT(182),
  [496] = {.entry = {.count = 1, .reusable = false}}, SHIFT(48),
  [498] = {.entry = {.count = 1, .reusable = false}}, SHIFT(63),
  [500] = {.entry = {.count = 1, .reusable = true}}, REDUCE(sym_argument_list, 1, 0, 0),
  [502] = {.entry = {.count = 1, .reusable = true}}, REDUCE(sym_type, 1, 0, 0),
  [504] = {.entry = {.count = 1, .reusable = true}}, REDUCE(sym_parameter_list, 2, 0, 0),
  [506] = {.entry = {.count = 1, .reusable = true}}, SHIFT(182),
  [508] = {.entry = {.count = 1, .reusable = false}}, SHIFT(100),
  [510] = {.entry = {.count = 1, .reusable = true}}, REDUCE(aux_sym_argument_list_repeat1, 2, 0, 0),
  [512] = {.entry = {.count = 2, .reusable = true}}, REDUCE(aux_sym_argument_list_repeat1, 2, 0, 0), SHIFT_REPEAT(67),
  [515] = {.entry = {.count = 1, .reusable = true}}, REDUCE(sym_argument_list, 2, 0, 0),
  [517] = {.entry = {.count = 1, .reusable = true}}, SHIFT(67),
  [519] = {.entry = {.count = 2, .reusable = false}}, REDUCE(aux_sym_argument_list_repeat1, 2, 0, 0), SHIFT_REPEAT(63),
  [522] = {.entry = {.count = 1, .reusable = true}}, REDUCE(sym_type_terminal, 1, 0, 0),
  [524] = {.entry = {.count = 1, .reusable = true}}, REDUCE(sym_type_definition, 2, 0, 0),
  [526] = {.entry = {.count = 1, .reusable = true}}, SHIFT(210),
  [528] = {.entry = {.count = 1, .reusable = false}}, REDUCE(sym_type, 1, 0, 0),
  [530] = {.entry = {.count = 1, .reusable = false}}, SHIFT(200),
  [532] = {.entry = {.count = 1, .reusable = false}}, REDUCE(sym_type_definition, 2, 0, 0),
  [534] = {.entry = {.count = 1, .reusable = false}}, REDUCE(sym_type_terminal, 1, 0, 0),
  [536] = {.entry = {.count = 1, .reusable = false}}, SHIFT(62),
  [538] = {.entry = {.count = 1, .reusable = true}}, REDUCE(sym_parameter_list, 1, 0, 0),
  [540] = {.entry = {.count = 1, .reusable = true}}, SHIFT(194),
  [542] = {.entry = {.count = 1, .reusable = true}}, SHIFT(261),
  [544] = {.entry = {.count = 1, .reusable = true}}, SHIFT(260),
  [546] = {.entry = {.count = 1, .reusable = true}}, SHIFT(268),
  [548] = {.entry = {.count = 1, .reusable = true}}, SHIFT(16),
  [550] = {.entry = {.count = 1, .reusable = true}}, SHIFT(185),
  [552] = {.entry = {.count = 1, .reusable = true}}, SHIFT(5),
  [554] = {.entry = {.count = 1, .reusable = true}}, SHIFT(237),
  [556] = {.entry = {.count = 1, .reusable = true}}, SHIFT(301),
  [558] = {.entry = {.count = 1, .reusable = true}}, SHIFT(71),
  [560] = {.entry = {.count = 1, .reusable = true}}, SHIFT(76),
  [562] = {.entry = {.count = 1, .reusable = true}}, SHIFT(104),
  [564] = {.entry = {.count = 1, .reusable = true}}, SHIFT(105),
  [566] = {.entry = {.count = 1, .reusable = true}}, SHIFT(65),
  [568] = {.entry = {.count = 1, .reusable = true}}, SHIFT(54),
  [570] = {.entry = {.count = 1, .reusable = true}}, SHIFT(96),
  [572] = {.entry = {.count = 1, .reusable = true}}, SHIFT(110),
  [574] = {.entry = {.count = 1, .reusable = true}}, SHIFT(7),
  [576] = {.entry = {.count = 1, .reusable = false}}, SHIFT(123),
  [578] = {.entry = {.count = 1, .reusable = true}}, SHIFT(330),
  [580] = {.entry = {.count = 1, .reusable = true}}, SHIFT(11),
  [582] = {.entry = {.count = 1, .reusable = true}}, SHIFT(145),
  [584] = {.entry = {.count = 1, .reusable = true}}, REDUCE(sym_parameter, 3, 0, 0),
  [586] = {.entry = {.count = 1, .reusable = true}}, SHIFT(180),
  [588] = {.entry = {.count = 1, .reusable = true}}, SHIFT(244),
  [590] = {.entry = {.count = 1, .reusable = true}}, SHIFT(15),
  [592] = {.entry = {.count = 1, .reusable = false}}, REDUCE(aux_sym_argument_list_repeat1, 2, 0, 0),
  [594] = {.entry = {.count = 1, .reusable = true}}, SHIFT(6),
  [596] = {.entry = {.count = 1, .reusable = true}}, SHIFT(8),
  [598] = {.entry = {.count = 1, .reusable = true}}, SHIFT(271),
  [600] = {.entry = {.count = 1, .reusable = true}}, SHIFT(56),
  [602] = {.entry = {.count = 1, .reusable = true}}, SHIFT(284),
  [604] = {.entry = {.count = 1, .reusable = true}}, REDUCE(sym_subroutine, 7, 0, 0),
  [606] = {.entry = {.count = 1, .reusable = true}}, REDUCE(sym_function, 7, 0, 0),
  [608] = {.entry = {.count = 1, .reusable = true}}, SHIFT(281),
  [610] = {.entry = {.count = 1, .reusable = true}}, SHIFT(191),
  [612] = {.entry = {.count = 1, .reusable = true}}, REDUCE(sym_if_statement, 6, 0, 0),
  [614] = {.entry = {.count = 1, .reusable = true}}, SHIFT(264),
  [616] = {.entry = {.count = 1, .reusable = true}}, SHIFT(265),
  [618] = {.entry = {.count = 1, .reusable = true}}, SHIFT(267),
  [620] = {.entry = {.count = 1, .reusable = true}}, SHIFT(258),
  [622] = {.entry = {.count = 1, .reusable = true}}, SHIFT(199),
  [624] = {.entry = {.count = 1, .reusable = true}}, REDUCE(sym_while_statement, 5, 0, 0),
  [626] = {.entry = {.count = 1, .reusable = true}}, SHIFT(309),
  [628] = {.entry = {.count = 1, .reusable = true}}, SHIFT(50),
  [630] = {.entry = {.count = 1, .reusable = true}}, REDUCE(sym_ptrsafe_function_declaration, 12, 0, 0),
  [632] = {.entry = {.count = 1, .reusable = true}}, REDUCE(sym_variable_list, 3, 0, 0),
  [634] = {.entry = {.count = 1, .reusable = true}}, REDUCE(sym_ptrsafe_function_declaration, 13, 0, 0),
  [636] = {.entry = {.count = 1, .reusable = true}}, SHIFT(274),
  [638] = {.entry = {.count = 1, .reusable = true}}, REDUCE(sym_function, 8, 0, 0),
  [640] = {.entry = {.count = 1, .reusable = true}}, SHIFT(256),
  [642] = {.entry = {.count = 1, .reusable = true}}, SHIFT(294),
  [644] = {.entry = {.count = 1, .reusable = true}}, REDUCE(sym_subroutine, 8, 0, 0),
  [646] = {.entry = {.count = 1, .reusable = true}}, SHIFT(196),
  [648] = {.entry = {.count = 1, .reusable = true}}, REDUCE(sym_modifier, 1, 0, 0),
  [650] = {.entry = {.count = 1, .reusable = true}}, SHIFT(33),
  [652] = {.entry = {.count = 1, .reusable = true}}, SHIFT(42),
  [654] = {.entry = {.count = 1, .reusable = true}}, SHIFT(295),
  [656] = {.entry = {.count = 1, .reusable = true}}, SHIFT(249),
  [658] = {.entry = {.count = 1, .reusable = true}}, SHIFT(177),
  [660] = {.entry = {.count = 1, .reusable = true}}, SHIFT(320),
  [662] = {.entry = {.count = 1, .reusable = true}}, SHIFT(248),
  [664] = {.entry = {.count = 1, .reusable = true}}, REDUCE(sym_function, 9, 0, 0),
  [666] = {.entry = {.count = 1, .reusable = true}}, REDUCE(sym_if_statement, 9, 0, 0),
  [668] = {.entry = {.count = 1, .reusable = true}}, REDUCE(sym_invocation_statement, 2, 0, 0),
  [670] = {.entry = {.count = 1, .reusable = true}}, SHIFT(35),
  [672] = {.entry = {.count = 1, .reusable = true}}, SHIFT(292),
  [674] = {.entry = {.count = 1, .reusable = true}}, REDUCE(sym_variable_declaration, 2, 0, 0),
  [676] = {.entry = {.count = 1, .reusable = true}}, SHIFT(113),
  [678] = {.entry = {.count = 1, .reusable = true}}, SHIFT(290),
  [680] = {.entry = {.count = 1, .reusable = true}}, SHIFT(321),
  [682] = {.entry = {.count = 1, .reusable = true}}, SHIFT(232),
  [684] = {.entry = {.count = 1, .reusable = true}}, SHIFT(181),
  [686] = {.entry = {.count = 1, .reusable = true}}, SHIFT(93),
  [688] = {.entry = {.count = 1, .reusable = true}}, SHIFT(179),
  [690] = {.entry = {.count = 1, .reusable = true}}, SHIFT(119),
  [692] = {.entry = {.count = 1, .reusable = true}}, SHIFT(120),
  [694] = {.entry = {.count = 1, .reusable = true}}, SHIFT(144),
  [696] = {.entry = {.count = 1, .reusable = true}}, REDUCE(sym_exit_statement, 2, 0, 0),
  [698] = {.entry = {.count = 1, .reusable = true}}, SHIFT(146),
  [700] = {.entry = {.count = 1, .reusable = true}}, SHIFT(61),
  [702] = {.entry = {.count = 1, .reusable = true}}, SHIFT(178),
  [704] = {.entry = {.count = 1, .reusable = true}}, SHIFT(323),
  [706] = {.entry = {.count = 1, .reusable = true}},  ACCEPT_INPUT(),
  [708] = {.entry = {.count = 1, .reusable = true}}, SHIFT(70),
  [710] = {.entry = {.count = 1, .reusable = false}}, SHIFT(297),
  [712] = {.entry = {.count = 1, .reusable = true}}, SHIFT(81),
  [714] = {.entry = {.count = 1, .reusable = true}}, SHIFT(83),
  [716] = {.entry = {.count = 1, .reusable = true}}, REDUCE(sym_for_statement, 10, 0, 0),
  [718] = {.entry = {.count = 1, .reusable = true}}, REDUCE(sym_function, 10, 0, 0),
  [720] = {.entry = {.count = 1, .reusable = true}}, REDUCE(sym_ptrsafe_function_declaration, 10, 0, 0),
  [722] = {.entry = {.count = 1, .reusable = true}}, SHIFT(328),
  [724] = {.entry = {.count = 1, .reusable = true}}, SHIFT(262),
  [726] = {.entry = {.count = 1, .reusable = false}}, SHIFT(300),
  [728] = {.entry = {.count = 1, .reusable = true}}, REDUCE(sym_array_element, 4, 0, 0),
  [730] = {.entry = {.count = 1, .reusable = true}}, SHIFT(52),
  [732] = {.entry = {.count = 1, .reusable = true}}, REDUCE(sym_for_statement, 12, 0, 0),
  [734] = {.entry = {.count = 1, .reusable = true}}, SHIFT(223),
  [736] = {.entry = {.count = 1, .reusable = false}}, SHIFT(312),
  [738] = {.entry = {.count = 1, .reusable = true}}, REDUCE(sym_ptrsafe_function_declaration, 11, 0, 0),
  [740] = {.entry = {.count = 1, .reusable = true}}, SHIFT(57),
  [742] = {.entry = {.count = 1, .reusable = true}}, SHIFT(327),
  [744] = {.entry = {.count = 1, .reusable = true}}, SHIFT(332),
};

#ifdef __cplusplus
extern "C" {
#endif
#ifdef TREE_SITTER_HIDE_SYMBOLS
#define TS_PUBLIC
#elif defined(_WIN32)
#define TS_PUBLIC __declspec(dllexport)
#else
#define TS_PUBLIC __attribute__((visibility("default")))
#endif

TS_PUBLIC const TSLanguage *tree_sitter_vbscript(void) {
  static const TSLanguage language = {
    .version = LANGUAGE_VERSION,
    .symbol_count = SYMBOL_COUNT,
    .alias_count = ALIAS_COUNT,
    .token_count = TOKEN_COUNT,
    .external_token_count = EXTERNAL_TOKEN_COUNT,
    .state_count = STATE_COUNT,
    .large_state_count = LARGE_STATE_COUNT,
    .production_id_count = PRODUCTION_ID_COUNT,
    .field_count = FIELD_COUNT,
    .max_alias_sequence_length = MAX_ALIAS_SEQUENCE_LENGTH,
    .parse_table = &ts_parse_table[0][0],
    .small_parse_table = ts_small_parse_table,
    .small_parse_table_map = ts_small_parse_table_map,
    .parse_actions = ts_parse_actions,
    .symbol_names = ts_symbol_names,
    .symbol_metadata = ts_symbol_metadata,
    .public_symbol_map = ts_symbol_map,
    .alias_map = ts_non_terminal_alias_map,
    .alias_sequences = &ts_alias_sequences[0][0],
    .lex_modes = ts_lex_modes,
    .lex_fn = ts_lex,
    .primary_state_ids = ts_primary_state_ids,
  };
  return &language;
}
#ifdef __cplusplus
}
#endif
