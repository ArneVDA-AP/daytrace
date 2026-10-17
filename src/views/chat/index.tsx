import { LegacyFrame } from "@/app/LegacyFrame";
import { useViewProps } from "@/app/view-props";
import { PageHeader } from "@/components/page-header";
import { ChatView } from "@/views/ChatView";

/* INTERIM (owned by the Chat engineer): the old Chat view inside the new shell. */
export default function ChatSection() {
  const props = useViewProps();
  return (
    <>
      <PageHeader title="Chat" />
      <LegacyFrame>
        <ChatView {...props} />
      </LegacyFrame>
    </>
  );
}
