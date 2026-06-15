module Example.Domain

open System

type Priority = Low | Medium | High

type Task = {
    Id: Guid
    Title: string
    Priority: Priority
    Done: bool
}

let createTask title priority =
    { Id = Guid.NewGuid(); Title = title; Priority = priority; Done = false }

let completeTask task =
    { task with Done = true }

let filterByPriority priority tasks =
    tasks |> List.filter (fun t -> t.Priority = priority)

let sortByPriority tasks =
    tasks |> List.sortByDescending (fun t ->
        match t.Priority with
        | High   -> 2
        | Medium -> 1
        | Low    -> 0)

let pendingCount tasks =
    tasks |> List.filter (fun t -> not t.Done) |> List.length
