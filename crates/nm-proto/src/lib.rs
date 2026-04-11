// Re-export all generated protobuf types

pub mod agent {
    tonic::include_proto!("neuralmesh.agent.v1");
}

pub mod job {
    tonic::include_proto!("neuralmesh.job.v1");
}

pub mod ledger {
    tonic::include_proto!("neuralmesh.ledger.v1");
}
