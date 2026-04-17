import grpc
import logging
import aether_pb2
import aether_pb2_grpc
from rag.pipeline import RAGPipeline

logger = logging.getLogger(__name__)

class RagServices(aether_pb2_grpc.RagServiceServicer):
    def __init__(self):
        self.pipeline= RAGPipeline()
        logger.info("RAG Pipeline initialized.")

    async def HealthCheck(self, request, context):
        logger.info(f"Health check ping received from: {request.caller}")
        return aether_pb2.HealthResponse(
            healthy=True,
            version="1.0.0",
            model="gpt-4o-mini"
        )

    async def Generate(self, request,context):
        logger.info(f"Generate Request [{request.request_id}]: {request.prompt}")
        full_text=""
        try:
            async for token in self.pipeline.generate_stream(request.prompt):
                full_text+=token

                yield aether_pb2.GenerateResponse(
                    token=token,
                    is_final=False,
                    request_id=request.request_id,
                    full_text=""
                )

            yield aether_pb2.GenerateResponse(
                token="",
                is_final=True,
                request_id=request.request_id,
                full_text=full_text
            )

            logger.info(f"Generation complete for [{request.request_id}]")

        except Exception as e:
            logger.error(f"LLM Generation Error: {str(e)}")
            context.set_code(grpc.StatusCode.INTERNAL)
            context.set_details(f"Python Brain Error: {str(e)}")
            raise   