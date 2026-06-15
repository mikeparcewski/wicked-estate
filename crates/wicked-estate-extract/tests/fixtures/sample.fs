module CodeIntel.Analysis

open System
open System.Collections.Generic

type Severity =
    | Info
    | Warning
    | Error

type Finding = {
    Rule: string
    Message: string
    Severity: Severity
    Line: int
}

let private severityWeight severity =
    match severity with
    | Info    -> 0
    | Warning -> 1
    | Error   -> 2

let scoreFindings (findings: Finding list) : int =
    findings
    |> List.map (fun f -> severityWeight f.Severity)
    |> List.sum

let filterBySeverity (minSeverity: Severity) (findings: Finding list) : Finding list =
    findings
    |> List.filter (fun f -> severityWeight f.Severity >= severityWeight minSeverity)

let formatReport (findings: Finding list) : string =
    let score = scoreFindings findings
    let count = List.length findings
    sprintf "findings=%d score=%d" count score

let runAnalysis (rules: string list) (lines: string list) : string =
    let findings =
        lines
        |> List.mapi (fun i line ->
            rules
            |> List.choose (fun rule ->
                if line.Contains(rule) then
                    Some { Rule = rule; Message = sprintf "match at line %d" (i + 1); Severity = Warning; Line = i + 1 }
                else
                    None))
        |> List.concat
    findings |> filterBySeverity Warning |> formatReport
