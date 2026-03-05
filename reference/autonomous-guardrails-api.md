# Autonomous Guardrails API Contract (H-01)

This contract defines the minimal blast-radius controls for autonomous operations in `voltnuerongridd`.

## Modes

- `disabled` - autonomous actions are denied.
- `advisory` - low-risk advisory actions allowed.
- `supervised` - medium/high-risk actions allowed with supervision.
- `autonomous` - full autonomous actions allowed.

Runtime configuration:

- `VNG_AUTONOMOUS_MODE` (default: `supervised`)
- `VNG_AUTONOMOUS_EMERGENCY_STOP` (default: `false`)

## Endpoints

### `GET /api/v1/autonomous/guardrails`

Returns current autonomous mode, emergency-stop state, and policy matrix.

Example response:

```json
{
  "status": "ok",
  "autonomous_mode": "supervised",
  "emergency_stop_enabled": false,
  "policy_matrix": [
    {
      "action": "schema_change",
      "required_mode": "supervised",
      "scope": "database",
      "rationale": "DDL and schema drift changes require human oversight"
    }
  ]
}
```

### `POST /api/v1/autonomous/emergency-stop`

Enable or disable emergency-stop.

Request:

```json
{
  "enabled": true,
  "reason": "security_incident",
  "requested_by": "sre_oncall"
}
```

Response:

```json
{
  "status": "ok",
  "emergency_stop_enabled": true,
  "reason": "security_incident",
  "requested_by": "sre_oncall"
}
```

### `POST /api/v1/autonomous/actions/authorize`

Evaluates a requested autonomous action against policy matrix and emergency-stop controls.

Request:

```json
{
  "action": "schema_change",
  "scope": "database"
}
```

Responses:

- `200` allow
- `403` denied by mode/policy
- `503` blocked by emergency-stop
- `404` unknown action

## Default allow/deny policy matrix

| Action | Required Mode | Scope | Outcome |
|---|---|---|---|
| `performance_tune` | `advisory` | `session` | allow in advisory/supervised/autonomous |
| `schema_change` | `supervised` | `database` | deny in disabled/advisory |
| `plugin_install` | `supervised` | `cluster` | deny in disabled/advisory |
| `security_patch` | `supervised` | `cluster` | deny in disabled/advisory |
| `self_heal_failover` | `autonomous` | `cluster` | allow only in autonomous |

If emergency-stop is enabled, all actions are blocked regardless of mode.
