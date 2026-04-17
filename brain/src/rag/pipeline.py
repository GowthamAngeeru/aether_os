import os
from langchain_openai import ChatOpenAI
from langchain_core.prompts import ChatPromptTemplate

class RAGPipeline:
    def __init__(self):
        model_name = os.getenv("OPENAI_MODEL", "gpt-4o-mini")
        temperature = float(os.getenv("TEMPERATURE", "0.1"))
        max_tokens = int(os.getenv("MAX_TOKENS", "1024")) 

        self.llm = ChatOpenAI(
            model=model_name,
            temperature=temperature,
            max_tokens=max_tokens,
            streaming=True
        )

        self.prompt = ChatPromptTemplate.from_messages([
            ("system", "You are Aether, an elite AI orchestrator. Provide clear, concise, and highly technical answers. Never apologize. Just deliver the data."),
            ("user", "{question}")
        ])

        self.chain = self.prompt | self.llm

    async def generate_stream(self, question: str):
        async for chunk in self.chain.astream({"question": question}):
            if chunk.content:
                yield chunk.content