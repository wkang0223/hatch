// Re-export all generated protobuf types

pub mod agent {
    tonic::include_proto!("hatch.agent.v1");
}

pub mod job {
    tonic::include_proto!("hatch.job.v1");
}

pub mod ledger {
    tonic::include_proto!("hatch.ledger.v1");
}
