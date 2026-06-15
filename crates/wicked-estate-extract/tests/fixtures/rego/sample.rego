package authz

import future.keywords.if
import future.keywords.in

default allow := false

allow if {
    is_authenticated
    has_permission(input.user, input.action, input.resource)
}

is_authenticated if {
    input.token != ""
    valid_token(input.token)
}

has_permission(user, action, resource) if {
    role := data.roles[user]
    permission := data.permissions[role][_]
    permission.action == action
    permission.resource == resource
}

valid_token(token) if {
    token in data.valid_tokens
}

deny[msg] if {
    not allow
    msg := sprintf("user %v denied access to %v", [input.user, input.resource])
}
