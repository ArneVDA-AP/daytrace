import { LegacyFrame } from "@/app/LegacyFrame";
import { useAppState } from "@/app/state";
import { PageHeader } from "@/components/page-header";
import { navigate } from "@/lib/router";
import { SettingsView } from "@/SettingsView";

/*
 * INTERIM (owned by the Settings engineer): the old one-page settings form inside the new shell.
 * Deep links already in use: #/settings?section=provider (banners, command menu).
 */
export default function SettingsSection() {
  const app = useAppState();
  return (
    <>
      <PageHeader title="Settings" />
      <LegacyFrame>
        <SettingsView
          dto={app.settings}
          onSaved={(dto) => {
            app.setSettings(dto);
            void app.reloadStatus();
          }}
          onClose={() => (history.length > 1 ? history.back() : navigate("timeline"))}
        />
      </LegacyFrame>
    </>
  );
}
