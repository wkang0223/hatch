// NATS JetStream job queue — pub/sub for job lifecycle events.
// Subjects:
//   nm.jobs.new          → new job submitted (matching engine subscribes)
//   nm.provider.<id>     → job assigned to specific provider
//   nm.consumer.<id>     → job status update to specific consumer
//   nm.jobs.heartbeat    → provider heartbeat fan-in
