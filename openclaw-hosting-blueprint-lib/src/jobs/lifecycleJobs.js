export const LIFECYCLE_JOB_TYPES = Object.freeze({
  CREATE: "create-hosted-instance",
  START: "start-hosted-instance",
  STOP: "stop-hosted-instance",
  DELETE: "delete-hosted-instance"
});

export function registerLifecycleJobs(hostedInstanceService) {
  return {
    [LIFECYCLE_JOB_TYPES.CREATE]: async (payload) =>
      hostedInstanceService.createHostedInstance(payload),
    [LIFECYCLE_JOB_TYPES.START]: async (payload) =>
      hostedInstanceService.startHostedInstance(payload),
    [LIFECYCLE_JOB_TYPES.STOP]: async (payload) =>
      hostedInstanceService.stopHostedInstance(payload),
    [LIFECYCLE_JOB_TYPES.DELETE]: async (payload) =>
      hostedInstanceService.deleteHostedInstance(payload)
  };
}
