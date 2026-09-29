import { Toaster as BaseToaster } from "@/components/ui/toast"

/**
 * Mounts the toast provider, portal and viewport. The `duration` of a toast is
 * set per toast by Base UI's provider default (5s), so no prop is threaded here.
 */
export function Toaster() {
  return <BaseToaster />
}
