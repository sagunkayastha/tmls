# tmls-web in a container: the browser version only. No tmux inside: every host, including the
# machine running the container, is reached over ssh (see deploy/ and README "Docker").
FROM python:3.12-slim
RUN apt-get update && apt-get install -y --no-install-recommends openssh-client \
    && rm -rf /var/lib/apt/lists/*
# Same uid as the user whose ssh key and config files are mounted in, so ssh accepts them.
RUN useradd -m -u 1000 tmls
WORKDIR /src
COPY pyproject.toml LICENSE ./
COPY src ./src
RUN pip install --no-cache-dir .
USER tmls
WORKDIR /home/tmls
ENTRYPOINT ["tmls-web"]
CMD ["--bind", "0.0.0.0", "--port", "8794"]
