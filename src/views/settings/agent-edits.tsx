import * as React from "react";
import { useAppState } from "@/app/state";
import { Switch } from "@/components/ui/switch";
import { errorText } from "@/lib/utils";
import { FieldError, Group, Row } from "./fields";
import { useSave } from "./save";

/**
 * Settings > AI tools: "Allow edits" (Dayflow's Settings > MCP / CLI toggle). Off by default; while
 * it is off, daytrace-mcp lists no write tools and the app refuses every change it is asked for.
 */
export function AgentEdits() {
  const app = useAppState();
  const save = useSave();
  const id = React.useId();
  const [error, setError] = React.useState<string | null>(null);
  const on = app.settings.settings.allowAgentEdits ?? false;

  return (
    <Group title="Changes by AI tools">
      <Row
        label="Allow edits"
        htmlFor={`${id}-edits`}
        description={
          <>
            <p>
              Lets connected AI tools rename, recategorize and delete activities, add, change and remove categories, and set day goals. Changes go through
              Daytrace, so it has to be open. Off: AI tools can only read.
            </p>
            <p>
              Every change is written to <span className="font-mono text-caption">agent-writes.log</span> in the data folder. AI tools started before you change
              this see the new setting in their next session.
            </p>
          </>
        }
      >
        <Switch
          id={`${id}-edits`}
          checked={on}
          onCheckedChange={(v) => {
            setError(null);
            save({ allowAgentEdits: v }).catch((e) => setError(`Couldn’t save: ${errorText(e)}`));
          }}
        />
      </Row>
      {error && (
        <div className="px-4 py-3">
          <FieldError>{error}</FieldError>
        </div>
      )}
    </Group>
  );
}
