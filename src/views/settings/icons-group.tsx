import * as React from "react";
import { useAppSettings } from "@/app/state";
import { Switch } from "@/components/ui/switch";
import { FieldError, Group, Row } from "./fields";
import { AppearanceSection } from "./general-sections";
import { useFieldSaves } from "./save";

/** Settings > Appearance: the theme, then the icons. */
export function AppearanceWithIcons() {
  return (
    <>
      <AppearanceSection />
      <IconsGroup />
    </>
  );
}

/** Settings > Appearance: app and site icons (features/icons.rs, components/app-mark.tsx). */
export function IconsGroup() {
  const s = useAppSettings().settings.settings;
  const fields = useFieldSaves();
  const id = React.useId();
  const show = fields.shown("icons", s.showIcons);
  return (
    <Group title="App and site icons" description="Shown next to app and site names on the timeline and in Daily and Weekly.">
      <Row
        label="Show app and site icons"
        htmlFor={`${id}-show`}
        description="App icons are read from the programs on this PC. Sites show their first letter unless site icons are looked up."
      >
        <Switch id={`${id}-show`} checked={show} onCheckedChange={(v) => void fields.apply("icons", v, { showIcons: v })} />
        <FieldError>{fields.error("icons")}</FieldError>
      </Row>
      <Row
        label="Look up site icons"
        htmlFor={`${id}-sites`}
        description={
          <>
            <p>
              Daytrace asks each website named on your timeline for its icon at <span translate="no">https://&lt;site&gt;/favicon.ico</span>, directly,
              without any other service in between. The website sees your IP address and a request for its icon; no cookies or other data are sent.
            </p>
            <p>Each site is asked once, and a site without an icon again after a week. Turning this off deletes the saved site icons.</p>
          </>
        }
      >
        <Switch
          id={`${id}-sites`}
          checked={fields.shown("sites", s.lookupSiteIcons)}
          disabled={!show}
          onCheckedChange={(v) => void fields.apply("sites", v, { lookupSiteIcons: v })}
        />
        <FieldError>{fields.error("sites")}</FieldError>
      </Row>
    </Group>
  );
}
