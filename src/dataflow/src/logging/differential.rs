// Copyright Materialize, Inc. All rights reserved.
//
// Use of this software is governed by the Business Source License
// included in the LICENSE file.
//
// As of the Change Date specified in that file, in accordance with
// the Business Source License, use of this software will be governed
// by the Apache License, Version 2.0.

//! Logging dataflows for events generated by differential dataflow.

use std::time::Duration;

use differential_dataflow::logging::DifferentialEvent;
use differential_dataflow::operators::count::CountTotal;
use timely::communication::Allocate;
use timely::dataflow::operators::capture::EventLink;
use timely::logging::WorkerIdentifier;

use super::{DifferentialLog, LogVariant};
use crate::arrangement::KeysValsHandle;
use repr::{Datum, RowPacker, Timestamp};

/// Constructs the logging dataflows and returns a logger and trace handles.
pub fn construct<A: Allocate>(
    worker: &mut timely::worker::Worker<A>,
    config: &dataflow_types::logging::LoggingConfig,
    linked: std::rc::Rc<EventLink<Timestamp, (Duration, WorkerIdentifier, DifferentialEvent)>>,
) -> std::collections::HashMap<LogVariant, (Vec<usize>, KeysValsHandle)> {
    let granularity_ms = std::cmp::max(1, config.granularity_ns / 1_000_000) as Timestamp;

    let traces = worker.dataflow(move |scope| {
        use differential_dataflow::collection::AsCollection;
        use timely::dataflow::operators::capture::Replay;
        use timely::dataflow::operators::Map;

        // TODO: Rewrite as one operator with multiple outputs.
        let logs = Some(linked).replay_core(
            scope,
            Some(Duration::from_nanos(config.granularity_ns as u64)),
        );

        // Duration statistics derive from the non-rounded event times.
        let arrangements = logs
            .flat_map(move |(ts, worker, event)| {
                let time_ms = ((ts.as_millis() as Timestamp / granularity_ms) + 1) * granularity_ms;
                match event {
                    DifferentialEvent::Batch(event) => {
                        let difference = differential_dataflow::difference::DiffVector::new(vec![
                            event.length as isize,
                            1,
                        ]);
                        Some(((event.operator, worker), time_ms, difference))
                    }
                    DifferentialEvent::Merge(event) => {
                        if let Some(done) = event.complete {
                            Some((
                                (event.operator, worker),
                                time_ms,
                                differential_dataflow::difference::DiffVector::new(vec![
                                    (done as isize) - ((event.length1 + event.length2) as isize),
                                    -1,
                                ]),
                            ))
                        } else {
                            None
                        }
                    }
                    DifferentialEvent::Drop(event) => {
                        let difference = differential_dataflow::difference::DiffVector::new(vec![
                            -(event.length as isize),
                            -1,
                        ]);
                        Some(((event.operator, worker), time_ms, difference))
                    }
                    DifferentialEvent::MergeShortfall(_) => None,
                    DifferentialEvent::TraceShare(_) => None,
                }
            })
            .as_collection()
            .count_total()
            .map({
                let mut row_packer = RowPacker::new();
                move |((op, worker), count)| {
                    row_packer.pack(&[
                        Datum::Int64(op as i64),
                        Datum::Int64(worker as i64),
                        Datum::Int64(count[0] as i64),
                        Datum::Int64(count[1] as i64),
                    ])
                }
            });

        // Duration statistics derive from the non-rounded event times.
        let sharing = logs
            .flat_map(move |(ts, worker, event)| {
                let time_ms = ((ts.as_millis() as Timestamp / granularity_ms) + 1) * granularity_ms;
                if let DifferentialEvent::TraceShare(event) = event {
                    Some(((event.operator, worker), time_ms, event.diff))
                } else {
                    None
                }
            })
            .as_collection()
            .count_total()
            .map({
                let mut row_packer = RowPacker::new();
                move |((op, worker), count)| {
                    row_packer.pack(&[
                        Datum::Int64(op as i64),
                        Datum::Int64(worker as i64),
                        Datum::Int64(count as i64),
                    ])
                }
            });

        let logs = vec![
            (
                LogVariant::Differential(DifferentialLog::Arrangement),
                arrangements,
            ),
            (LogVariant::Differential(DifferentialLog::Sharing), sharing),
        ];

        use differential_dataflow::operators::arrange::arrangement::ArrangeByKey;
        let mut result = std::collections::HashMap::new();
        for (variant, collection) in logs {
            if config.active_logs.contains_key(&variant) {
                let key = variant.index_by();
                let key_clone = key.clone();
                let trace = collection
                    .map({
                        let mut row_packer = RowPacker::new();
                        move |row| {
                            let datums = row.unpack();
                            let key_row = row_packer.pack(key.iter().map(|k| datums[*k]));
                            (key_row, row)
                        }
                    })
                    .arrange_by_key()
                    .trace;
                result.insert(variant, (key_clone, trace));
            }
        }
        result
    });

    traces
}
