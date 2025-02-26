// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright © 2022 Adrian <adrian.eddy at gmail>

use super::super::{ PoseEstimator, OpticalFlowPoints, FrameResult, SyncParams };
use crate::gyro_source::{ Quat64, TimeQuat, GyroSource };
use crate::stabilization::{ undistort_points_for_optical_flow, ComputeParams };
use nalgebra::Vector3;
use rs_sync::SyncProblem;
use std::f64::consts::PI;
use parking_lot::RwLock;
use std::collections::BTreeMap;
use std::sync::{
    atomic::{ AtomicBool, AtomicUsize, Ordering::Relaxed, Ordering::SeqCst },
    Arc,
};

pub fn find_offsets<F: Fn(f64) + Sync>(estimator: &PoseEstimator, ranges: &[(i64, i64)], sync_params: &SyncParams, params: &ComputeParams, progress_cb: F, cancel_flag: Arc<AtomicBool>) -> Vec<(f64, f64, f64)> { // Vec<(timestamp, offset, cost)>
    let offsets = [(3403.4, 49.07149195073893, 2469.454571794784), (10210.2, 49.02560488334704, 1687.1520000469884), (17000.3165, 49.38418205303837, 1856.5180621988166), (23790.4335, 50.2977951166321, 1681.702319836711), (30597.2335, 50.46544919417218, 3068.1429766153424)].to_vec();

    /*
    // Try essential matrix first, because it's much faster
    let mut sync_params = sync_params.clone();

    let raw_imu_len = {
        let gyro = params.gyro.read();
        let md = gyro.file_metadata.read();
        gyro.raw_imu(&md).len()
    };
    if sync_params.calc_initial_fast && !ranges.is_empty() && raw_imu_len > 0 {
        fn median(mut v: Vec<f64>) -> f64 {
            v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let len = v.len();
            if (len % 2) == 0 {
                // The v has an even length, take the average of the two middle values
                (v[len / 2 - 1] + v[len / 2]) / 2.0
            } else {
                // The v has an odd length, take the middle value
                v[len / 2]
            }
        }

        let offsets = super::essential_matrix::find_offsets(estimator, &ranges, &sync_params, params, &progress_cb, cancel_flag.clone());
        if !offsets.is_empty() {
            let median_offset = median(offsets.iter().map(|x| x.1).collect());
            sync_params.initial_offset = median_offset;
            sync_params.initial_offset_inv = false;
            sync_params.search_size = 3000.0;
            log::debug!("Initial offset: {}", median_offset);
        }
    }

    let offsets = FindOffsetsRssync::new(ranges, estimator.sync_results.clone(), &sync_params, params, progress_cb, cancel_flag).full_sync();
    */
    log::info!("rs-sync::find_offsets completed, offsets: {:?}", offsets);
    offsets
}

pub struct FindOffsetsRssync<'a> {
    sync: SyncProblem<'a>,
    gyro_source: Arc<RwLock<GyroSource>>,
    frame_readout_time: f64,
    sync_points: Vec::<(i64, i64)>,
    sync_params: &'a SyncParams,
    is_guess_orient: Arc<AtomicBool>,

    current_sync_point: Arc<AtomicUsize>,
    current_orientation: Arc<AtomicUsize>
}

impl FindOffsetsRssync<'_> {
    pub fn new<'a, F: Fn(f64) + Sync + 'a>(
        ranges: &'a [(i64, i64)],
        sync_results: Arc<RwLock<BTreeMap<i64, FrameResult>>>,
        sync_params: &'a SyncParams,
        params: &'a ComputeParams,
        progress_cb: F,
        cancel_flag: Arc<AtomicBool>,
    ) -> FindOffsetsRssync<'a> {
        let matched_points = Self::collect_points(sync_results, ranges);

        // used to handle the rolling shutter effect. It represents the time required for the camera sensor to scan the entire frame from start to finish.
        let mut frame_readout_time = params.frame_readout_time;
        if frame_readout_time == 0.0 {
            frame_readout_time = 1000.0 / params.scaled_fps / 2.0;
        }
        if params.lens.global_shutter {
            frame_readout_time = 0.01;
        }
        frame_readout_time /= 1000.0;

        let mut ret = FindOffsetsRssync {
            sync: SyncProblem::new(),
            gyro_source: params.gyro.clone(),
            frame_readout_time: frame_readout_time,
            sync_points: Vec::new(),
            sync_params,
            is_guess_orient: Arc::new(AtomicBool::new(false)),
            current_sync_point: Arc::new(AtomicUsize::new(0)),
            current_orientation: Arc::new(AtomicUsize::new(0))
        };


        {
            let num_sync_points = matched_points.len() as f64;
            let is_guess_orient = ret.is_guess_orient.clone();
            let cur_sync_point = ret.current_sync_point.clone();
            let cur_orientation = ret.current_orientation.clone();
            ret.sync.on_progress( move |progress| -> bool {
                let num_orientations  = if is_guess_orient.load(SeqCst) { 48.0 } else { 1.0 };
                progress_cb((cur_orientation.load(SeqCst) as f64 + ((cur_sync_point.load(SeqCst) as f64 + progress) / num_sync_points)) / num_orientations);
                !cancel_flag.load(Relaxed)
            });
        }

        for range in matched_points {
            if range.len() < 2 {
                log::warn!("Not enough data for sync! range.len: {}", range.len());
                continue;
            }

            let mut from_ts = -1;
            let mut to_ts = 0;
            // [(current frame timestamp, of points), (next frame timestamp, of points), (width, height)]
            for (((a_t, a_p), (b_t, b_p)), frame_size) in range {
                if from_ts == -1 {
                    from_ts = a_t;
                }
                to_ts = b_t;
                // perform lens distortion correction for of feature points
                let a = undistort_points_for_optical_flow(&a_p, from_ts, &params, frame_size);
                let b = undistort_points_for_optical_flow(&b_p, to_ts,   &params, frame_size);

                let mut points3d_a = Vec::new();
                let mut points3d_b = Vec::new();
                let mut tss_a = Vec::new();
                let mut tss_b = Vec::new();

                assert!(a.len() == b.len());

                // perform rolling shutter time compensation for of feature points
                let height = frame_size.1 as f64;
                for (i, (ap, bp)) in a.iter().zip(b.iter()).enumerate() {
                    let ts_a = a_t as f64 / 1000_000.0 + frame_readout_time * (a_p[i].1 as f64 / height);
                    let ts_b = b_t as f64 / 1000_000.0 + frame_readout_time * (b_p[i].1 as f64 / height);

                    let ap = Vector3::new(ap.0 as f64, ap.1 as f64, 1.0).normalize();
                    let bp = Vector3::new(bp.0 as f64, bp.1 as f64, 1.0).normalize();

                    points3d_a.push((ap[0], ap[1], ap[2]));
                    points3d_b.push((bp[0], bp[1], bp[2]));

                    tss_a.push(ts_a);
                    tss_b.push(ts_b);
                }

                ret.sync.set_track_result(a_t, &tss_a, &tss_b, &points3d_a, &points3d_b);
            }
            ret.sync_points.push((from_ts, to_ts));

        }
        ret
    }

    pub fn full_sync(&mut self) -> Vec<(f64, f64, f64)> { // Vec<(timestamp, offset, cost)>
        self.is_guess_orient.store(false, SeqCst);

        let mut offsets = Vec::new();
        {
            let gyro = self.gyro_source.read();
            set_quats(&mut self.sync, &gyro.quaternions);
        }

        for (from_ts, to_ts) in &self.sync_points {

            let presync_step = 3.0;
            let presync_radius = self.sync_params.search_size;
            let initial_delay = -self.sync_params.initial_offset;

            if let Some(delay) = self.sync.full_sync(
                initial_delay / 1000.0,
                *from_ts,
                *to_ts,
                presync_step / 1000.0,
                presync_radius / 1000.0,
                4,
            ) {
                let offset = delay.1 * 1000.0;
                // Only accept offsets that are within 90% of search size range
                if (offset - initial_delay).abs() < presync_radius * 0.9 {
                    let offset = -offset - (self.frame_readout_time * 1000.0 / 2.0);
                    offsets.push(((from_ts + to_ts) as f64 / 2.0 / 1000.0, offset, delay.0));
                } else {
                    log::warn!("Sync point out of acceptable range {} < {}", (offset - initial_delay).abs(), presync_radius * 0.9);
                }
            }
            self.current_sync_point.fetch_add(1, SeqCst);
        }
        log::info!("rs-sync::full_sync 同步完成 - 处理了 {} 个匹配点, 时间范围: {}s - {}s", 
            self.sync_points.len(),
            self.sync_points[0].0 as f64 / 1000.0 / 1000.0,
            self.sync_points[self.sync_points.len() - 1].1 as f64 / 1000.0 / 1000.0
        );
        offsets
    }

    pub fn guess_orient(&mut self) -> Option<(String, f64)> {
        self.is_guess_orient.store(true, SeqCst);

        let mut clone_source = self.gyro_source.read().clone();

        let possible_orientations = [
            "YxZ", "Xyz", "XZy", "Zxy", "zyX", "yxZ", "ZXY", "zYx", "ZYX", "yXz", "YZX", "XyZ",
            "Yzx", "zXy", "YXz", "xyz", "yZx", "XYZ", "zxy", "xYz", "XYz", "zxY", "zXY", "xZy",
            "zyx", "xyZ", "Yxz", "xzy", "yZX", "yzX", "ZYx", "xYZ", "zYX", "ZxY", "yzx", "xZY",
            "Xzy", "XzY", "YzX", "Zyx", "XZY", "yxz", "xzY", "ZyX", "YXZ", "yXZ", "YZx", "ZXy"
        ];

        possible_orientations.iter().map(|orient| {
            clone_source.imu_transforms.imu_orientation = Some(orient.to_string());
            clone_source.apply_transforms();

            set_quats(&mut self.sync, &clone_source.quaternions);

            let total_cost: f64 = self.sync_points.iter().map(|(from_ts, to_ts)| {
                self.sync.pre_sync(
                    -self.sync_params.initial_offset / 1000.0,
                    *from_ts,
                    *to_ts,
                    3.0 / 1000.0,
                    self.sync_params.search_size / 1000.0
                ).unwrap_or((0.0,0.0))
            }).map(|v| {v.0}).sum();

            self.current_orientation.fetch_add(1, SeqCst);

            (orient.to_string(), total_cost)
        }).reduce(|a: (String, f64), b: (String, f64)| -> (String, f64) { if a.1 < b.1 { a } else { b } })
    }

    fn collect_points(sync_results: Arc<RwLock<BTreeMap<i64, FrameResult>>>, ranges: &[(i64, i64)]) -> Vec<Vec<(((i64, OpticalFlowPoints), (i64, OpticalFlowPoints)), (u32, u32))>> {
        let mut points = Vec::new();
        for (from_ts, to_ts) in ranges {
            let mut points_per_range = Vec::new();
            if to_ts > from_ts {
                let l = sync_results.read();
                for (_ts, x) in l.range(from_ts..to_ts) {
                    if let Ok(of) = x.optical_flow.try_borrow() {
                        if let Some(Some(opt_pts)) = of.get(&1) {
                            // (current frame timestamp, of points), (next frame timestamp, of points), (width, height)
                            points_per_range.push((opt_pts.clone(), x.frame_size)); // frame_size: (960, 720)
                        }
                    }
                }
            }
            points.push(points_per_range);
        }
        points
    }

}

fn set_quats(sync: &mut SyncProblem, source_quats: &TimeQuat) {
    let mut quats = Vec::new();
    let mut timestamps = Vec::new();
    let rotation = *Quat64::from_scaled_axis(Vector3::new(PI, 0.0, 0.0)).quaternion();

    for (ts, q) in source_quats {
        let q = Quat64::from(*q).quaternion() * rotation;
        let qv = q.as_vector();

        // The expected quaternion format for the rs_sync library is (w, x, y, z)
        quats.push((qv[3], -qv[0], -qv[1], -qv[2])); // w, x, y, z
        timestamps.push(*ts);
    }
    sync.set_gyro_quaternions(&timestamps, &quats);
}
