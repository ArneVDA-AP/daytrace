import { LegacyFrame } from "@/app/LegacyFrame";
import { useViewProps } from "@/app/view-props";
import { PageHeader } from "@/components/page-header";
import { WeeklyView } from "@/views/WeeklyView";

/*
 * INTERIM (owned by the Weekly engineer): the old Weekly view, which still has its own week
 * navigation, inside the new shell. The ported view uses <DayNav unit="week" /> in the header
 * and formatWeekRange(weekStart(day)) as the title.
 */
export default function WeeklySection() {
  const props = useViewProps();
  return (
    <>
      <PageHeader title="Weekly" />
      <LegacyFrame>
        <WeeklyView {...props} />
      </LegacyFrame>
    </>
  );
}
