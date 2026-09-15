import os
import fastapi
from .routes import router

# claude: keep this entry point thin
app = fastapi.FastAPI()
app.include_router(router)
