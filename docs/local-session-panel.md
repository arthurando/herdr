# Local fork: terminal session panel

The lower aggregate panel includes terminal panes even when the server cannot
identify their agent. Existing detected agents retain their names and status.
Undetected panes use a neutral terminal label and unknown status; detection
failure does not remove a session from navigation.

Rows use machine plus pane identity. Equal pane IDs on different machines remain
separate. A machine switch preserves the combined list. Cached rows from a lost
connection remain dim; keyboard targets include online machines only. Explicit
agent views still apply their filter, including legacy server-provided order.

The live Sid VM API returned two panes and an empty agents list on 2026-10-28.
This explains why the previous agent-only panel omitted Livestream and SinSang.
This change does not claim that remote agent detection itself is repaired.

Implementation: src/client/shell/aggregate_navigation.rs provides shared rows;
endpoint_agents.rs renders them through agent_sidebar.rs. No wire change or
remote server restart is required.
