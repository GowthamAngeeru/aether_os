import http from "k6/http";
import { check, sleep } from "k6";
import { Rate, Trend, Counter } from "k6/metrics";

const cacheHitRate = new Rate("cache_hit_rate");
const cacheHitTrend = new Trend("cache_hit_latency_ms");
const totalRequests = new Counter("total_requests");

export const options = {
	scenarios: {
		warmup: {
			executor: "shared-iterations",
			vus: 1,
			iterations: 1,
			maxDuration: "10s",
		},
		load: {
			executor: "constant-vus",
			vus: 10,
			duration: "15s",
			startTime: "5s",
		},
	},
};

export default function () {
	const start = Date.now();

	const response = http.post(
		"http://localhost:3000/generate",
		JSON.stringify({ prompt: "What is AetherOS?" }),
		{
			headers: { "Content-Type": "application/json" },
			timeout: "10s",
		},
	);

	const latency = Date.now() - start;
	totalRequests.add(1);

	const isHit = response.body && response.body.includes("event: cache_hit");
	cacheHitRate.add(isHit);

	if (isHit) {
		cacheHitTrend.add(latency);
	}

	check(response, {
		"status is 200": (r) => r.status === 200,
	});

	sleep(0.1);
}

export function handleSummary(data) {
	const summary = {
		cache_hit_rate: data.metrics.cache_hit_rate?.values?.rate ?? 0,
		cache_hit_p95_ms: data.metrics.cache_hit_latency_ms?.values?.["p(95)"] ?? 0,
		cache_hit_avg_ms: data.metrics.cache_hit_latency_ms?.values?.avg ?? 0,
		total_requests: data.metrics.total_requests?.values?.count ?? 0,
	};

	console.log("\n=======================================");
	console.log("🚀 AetherOS Pure Cache Benchmark");
	console.log("=======================================");
	console.log(
		`🎯 Cache Hit Rate:   ${(summary.cache_hit_rate * 100).toFixed(1)}%`,
	);
	console.log(`⚡ Cache Hit Avg:    ${summary.cache_hit_avg_ms.toFixed(2)}ms`);
	console.log(`⚡ Cache Hit P95:    ${summary.cache_hit_p95_ms.toFixed(2)}ms`);
	console.log(`📈 Total Requests:   ${summary.total_requests}`);
	console.log("=======================================\n");
}
