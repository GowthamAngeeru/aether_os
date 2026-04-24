import os
import json
import asyncio
from fastapi import FastAPI,HTTPException
from fastapi.responses import StreamingResponse
from pydantic import BaseModel
from langchain_openai import ChatOpenAI
from langchain_core.messages import HumanMessage, SystemMessage
from dotenv import load_dotenv


load_dotenv()

app=FastAPI(title="AetherOS Brain", version="2.0.0")


class GenerateRequest(BaseModel):
    prompt: str
    request_id: str
    max_tokens: int = 1024
    context_chunks: list[str] = []

class HealthResponse(BaseModel):
    healthy: bool
    version: str
    model: str

@app.get("/health", response_model=HealthResponse)
async def health():
    return HealthResponse(
        healthy=True,
        version="2.0.0",
        model=os.getenv("OPENAI_MODEL", "gpt-4o-mini"),
    )

@app.post("/generate")
async def generate(request: GenerateRequest):
    if not request.prompt.strip():
        raise HTTPException(status_code=400, detail="Prompt cannot be empty")

    async def token_stream():
        try:
            llm = ChatOpenAI(
                model=os.getenv("OPENAI_MODEL", "gpt-4o-mini"),
                streaming=True,
                temperature=float(os.getenv("TEMPERATURE", "0.1")),
                max_tokens=request.max_tokens,
                api_key=os.getenv("OPENAI_API_KEY"),
            )

            context_text = "\n\n---\n\n".join(request.context_chunks)
            system_content = f"""You are AetherOS, an expert AI assistant with access to a curated knowledge base.
Use the following retrieved context to answer accurately.
If the context is insufficient, state this clearly.
Never hallucinate facts not present in the context.

Retrieved Context:
{context_text if context_text else "No context retrieved."}"""

            messages = [
                SystemMessage(content=system_content),
                HumanMessage(content=request.prompt),
            ]

            full_text = ""

            async for chunk in llm.astream(messages):
                token = chunk.content
                if token:
                    full_text += token
                    yield f"event: token\ndata: {token}\n\n"
                    await asyncio.sleep(0)

            final_payload = json.dumps({
                "full_text": full_text,
                "request_id": request.request_id,
            })
            yield f"event: done\ndata: {final_payload}\n\n"

        except Exception as e:
            error_payload = json.dumps({"error": str(e)})
            yield f"event: error\ndata: {error_payload}\n\n"

    return StreamingResponse(
        token_stream(),
        media_type="text/event-stream",
        headers={
            "Cache-Control": "no-cache",
            "X-Accel-Buffering": "no",
            "X-Request-Id": request.request_id,
        }
    )