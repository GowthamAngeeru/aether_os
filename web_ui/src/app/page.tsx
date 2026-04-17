"use client";

import { useState, useRef, useEffect, useCallback } from "react";
import ReactMarkdown from "react-markdown";

// ─── Types ────────────────────────────────────────────────────────────────────
interface Message {
	role: "ai" | "user";
	content: string;
	isCacheHit?: boolean;
	isError?: boolean;
}

// ─── Environment Config ───────────────────────────────────────────────────────
const API_BASE = process.env.NEXT_PUBLIC_API_URL ?? "http://localhost:3000";

// ─── SSE Parser ───────────────────────────────────────────────────────────────
interface SseFrame {
	event: string;
	data: string;
}

function parseSseFrames(buffer: string): {
	frames: SseFrame[];
	remaining: string;
} {
	const frames: SseFrame[] = [];
	const parts = buffer.split("\n\n");
	const remaining = parts.pop() ?? "";

	for (const part of parts) {
		const lines = part.split("\n");
		let event = "message";
		const dataLines: string[] = []; // Aggregates multi-line outputs (like Python code)

		for (const line of lines) {
			if (line.startsWith("event: ")) {
				event = line.slice(7).trim();
			} else if (line.startsWith("data: ")) {
				dataLines.push(line.slice(6));
			}
		}

		if (dataLines.length > 0) {
			frames.push({ event, data: dataLines.join("\n") }); // Rejoin with actual newlines
		}
	}

	return { frames, remaining };
}

// ─── Component ────────────────────────────────────────────────────────────────
export default function Home() {
	const [prompt, setPrompt] = useState("");
	const [messages, setMessages] = useState<Message[]>([
		{
			role: "ai",
			content:
				"System Online. AetherOS Knowledge Base active. Awaiting input...",
		},
	]);
	const [isGenerating, setIsGenerating] = useState(false);
	const chatEndRef = useRef<HTMLDivElement>(null);

	// Auto-scroll to the bottom of the chat
	useEffect(() => {
		chatEndRef.current?.scrollIntoView({ behavior: "smooth" });
	}, [messages]);

	const sendMessage = useCallback(async () => {
		if (!prompt.trim() || isGenerating) return;

		const userMsg = prompt.trim();
		setPrompt("");

		// 1. Add user message and empty AI placeholder
		setMessages((prev) => [
			...prev,
			{ role: "user", content: userMsg },
			{ role: "ai", content: "", isCacheHit: false },
		]);
		setIsGenerating(true);

		let sseBuffer = "";

		try {
			const response = await fetch(`${API_BASE}/generate`, {
				method: "POST",
				headers: { "Content-Type": "application/json" },
				body: JSON.stringify({ prompt: userMsg }),
			});

			if (!response.ok) {
				throw new Error(
					`Server error: ${response.status} ${response.statusText}`,
				);
			}

			if (!response.body) {
				throw new Error("No response body — server may not support streaming");
			}

			const reader = response.body.getReader();
			const decoder = new TextDecoder("utf-8");

			while (true) {
				const { done, value } = await reader.read();
				if (done) break;

				sseBuffer += decoder.decode(value, { stream: true });
				const { frames, remaining } = parseSseFrames(sseBuffer);
				sseBuffer = remaining;

				for (const frame of frames) {
					switch (frame.event) {
						case "cache_hit":
							setMessages((prev) => {
								const msgs = [...prev];
								msgs[msgs.length - 1] = {
									...msgs[msgs.length - 1],
									content: frame.data,
									isCacheHit: true,
								};
								return msgs;
							});
							break;

						case "token":
							setMessages((prev) => {
								const msgs = [...prev];
								msgs[msgs.length - 1] = {
									...msgs[msgs.length - 1],
									content: msgs[msgs.length - 1].content + frame.data,
								};
								return msgs;
							});
							break;

						case "done":
							break;

						case "error":
							setMessages((prev) => {
								const msgs = [...prev];
								msgs[msgs.length - 1] = {
									...msgs[msgs.length - 1],
									content: `Error: ${frame.data}`,
									isError: true,
								};
								return msgs;
							});
							break;
					}
				}
			}
		} catch (error) {
			const errorMsg =
				error instanceof Error ? error.message : "Unknown error occurred";
			setMessages((prev) => {
				const msgs = [...prev];
				msgs[msgs.length - 1] = {
					...msgs[msgs.length - 1],
					content: `Connection failed: ${errorMsg}\n\nIs the Rust server running on ${API_BASE}?`,
					isError: true,
				};
				return msgs;
			});
		} finally {
			setIsGenerating(false);
		}
	}, [prompt, isGenerating]);

	return (
		<main className="flex flex-col items-center justify-center min-h-screen bg-[#0d1117] text-[#c9d1d9] p-6 font-sans">
			<div className="w-full max-w-4xl flex flex-col h-screen py-6">
				{/* Header */}
				<div className="text-center mb-6">
					<h1 className="text-4xl font-bold text-[#58a6ff] tracking-tight">
						AetherOS
					</h1>
					<p className="text-[#8b949e] mt-2 font-mono text-sm">
						Rust Gateway ⚡ Qdrant RAG ⚡ Redis Semantic Cache
					</p>
					<div className="flex justify-center gap-2 mt-3">
						<StatusBadge label="Rust Edge" color="green" />
						<StatusBadge label="Qdrant DB" color="purple" />
						<StatusBadge label="Redis Cache" color="red" />
					</div>
				</div>

				{/* Chat Window */}
				<div className="flex-grow bg-[#161b22] border border-[#30363d] rounded-xl p-6 overflow-y-auto mb-4 flex flex-col gap-4 shadow-2xl">
					{messages.map((msg, idx) => (
						<MessageBubble key={idx} message={msg} />
					))}
					{isGenerating && (
						<div className="flex items-center gap-2 text-[#8b949e] text-sm self-start mt-2">
							<span className="inline-flex gap-1">
								<span className="animate-bounce">●</span>
								<span className="animate-bounce [animation-delay:0.1s]">●</span>
								<span className="animate-bounce [animation-delay:0.2s]">●</span>
							</span>
							<span className="font-mono">Processing...</span>
						</div>
					)}
					<div ref={chatEndRef} />
				</div>

				{/* Input Area */}
				<div className="flex gap-3">
					<input
						type="text"
						className="flex-grow p-4 bg-[#0d1117] border border-[#30363d] text-[#c9d1d9] rounded-lg focus:outline-none focus:border-[#58a6ff] transition-colors font-mono"
						placeholder="Ask about the AetherOS architecture..."
						value={prompt}
						onChange={(e) => setPrompt(e.target.value)}
						onKeyDown={(e) => e.key === "Enter" && !e.shiftKey && sendMessage()}
						disabled={isGenerating}
						autoFocus
					/>
					<button
						className="bg-[#238636] hover:bg-[#2ea043] disabled:bg-[#30363d] disabled:text-[#8b949e] disabled:cursor-not-allowed text-white px-8 py-4 rounded-lg font-bold transition-colors"
						onClick={sendMessage}
						disabled={isGenerating || !prompt.trim()}
					>
						{isGenerating ? "⏳" : "Send ↵"}
					</button>
				</div>

				<p className="text-center text-[#484f58] text-xs mt-3 font-mono">
					Connected to {API_BASE} · SSE Streaming Active
				</p>
			</div>
		</main>
	);
}

// ─── Sub-Components ───────────────────────────────────────────────────────────
function MessageBubble({ message }: { message: Message }) {
	const isUser = message.role === "user";

	return (
		<div className={`flex ${isUser ? "justify-end" : "justify-start"}`}>
			<div
				className={`p-4 rounded-lg max-w-[85%] leading-relaxed text-sm ${
					isUser
						? "bg-[#238636] text-white"
						: message.isError
							? "bg-[#3d1a1a] border border-[#f85149] text-[#f85149]"
							: "bg-[#21262d] border border-[#30363d] text-[#c9d1d9]"
				}`}
			>
				{message.isCacheHit && (
					<div className="flex items-center gap-2 mb-3">
						<span className="inline-block bg-[#e3b341] text-black text-xs font-bold px-2 py-1 rounded-full">
							⚡ Cache Hit
						</span>
						<span className="text-[#8b949e] text-xs font-mono">
							~3ms · $0.00 API cost
						</span>
					</div>
				)}

				{/* ReactMarkdown elegantly styles the code blocks and bold text */}
				{message.content ? (
					<ReactMarkdown
						components={{
							p: ({ node, ...props }) => (
								<p className="mb-3 last:mb-0" {...props} />
							),
							pre: ({ node, ...props }) => (
								<pre
									className="bg-[#0d1117] p-4 rounded-md my-3 overflow-x-auto font-mono text-sm border border-[#30363d]"
									{...props}
								/>
							),
							code({ node, className, children, ...props }) {
								const isInline = !className?.includes("language-");
								return isInline ? (
									<code
										className="bg-[#0d1117] px-1.5 py-0.5 rounded-md text-[#58a6ff] font-mono text-[0.85em]"
										{...props}
									>
										{children}
									</code>
								) : (
									<code className={`${className} font-mono text-sm`} {...props}>
										{children}
									</code>
								);
							},
							ul: ({ node, ...props }) => (
								<ul className="list-disc pl-5 mb-3 space-y-1" {...props} />
							),
							ol: ({ node, ...props }) => (
								<ol className="list-decimal pl-5 mb-3 space-y-1" {...props} />
							),
							strong: ({ node, ...props }) => (
								<strong className="font-bold text-white" {...props} />
							),
						}}
					>
						{message.content}
					</ReactMarkdown>
				) : (
					<span className="text-[#484f58] italic">Waiting for response...</span>
				)}
			</div>
		</div>
	);
}

function StatusBadge({
	label,
	color,
}: {
	label: string;
	color: "green" | "red" | "purple";
}) {
	const colors = {
		green: "bg-[#1a2e1a] border-[#238636] text-[#3fb950]",
		red: "bg-[#2a1a1a] border-[#da3633] text-[#f85149]",
		purple: "bg-[#1e1a2e] border-[#8957e5] text-[#bc8cff]",
	};

	return (
		<span
			className={`inline-flex items-center gap-1 text-xs font-mono px-2 py-1 rounded border ${colors[color]}`}
		>
			<span className="w-1.5 h-1.5 rounded-full bg-current animate-pulse" />
			{label}
		</span>
	);
}
